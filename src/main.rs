//! Sail: a graphical UEFI boot menu. See the README for what it does and how to install it.
#![no_main]
#![no_std]

extern crate alloc;

mod entries;
mod gfx;
mod icons;
mod store;
mod text;
mod tools;
mod ui;

use alloc::string::String;
use core::time::Duration;
use entries::get_protocol;
use sail::nav::{Dir, Nav, countdown_step};
use uefi::boot::{self, EventType, TimerTrigger, Tpl};
use uefi::prelude::*;
use uefi::proto::console::gop::GraphicsOutput;
use uefi::proto::console::text::{Key, ScanCode};
use uefi::system;
use ui::{COUNTDOWN_TENTHS, Ui};

#[used]
#[unsafe(link_section = ".sbat")]
static SBAT: [u8; 130] = *b"sbat,1,SBAT Version,sbat,1,https://github.com/rhboot/shim/blob/main/SBAT.md\nsail,1,Sail,sail,0.1.0,https://github.com/fr3on/sail\n\0";

const TICK: Duration = Duration::from_millis(100);

#[entry]
fn main() -> Status {
    if uefi::helpers::init().is_err() {
        return Status::LOAD_ERROR;
    }
    // The firmware arms a 5 minute watchdog for boot managers; a menu that waits for a
    // person must not reboot the machine under them.
    let _ = boot::set_watchdog_timer(0, 0x1_0000, None);

    let items = entries::discover();
    let gop = boot::get_handle_for_protocol::<GraphicsOutput>()
        .ok()
        .and_then(get_protocol::<GraphicsOutput>);
    match (gop, gfx::Fonts::load()) {
        (Some(mut gop), Some(fonts)) => {
            ui::pick_best_mode(&mut gop);
            graphical(gop, fonts, items)
        }
        // No usable graphics: do not panic, offer a plain list instead.
        _ => text::menu(&items),
    }
}

fn graphical(
    gop: uefi::boot::ScopedProtocol<GraphicsOutput>,
    fonts: gfx::Fonts,
    items: alloc::vec::Vec<entries::Entry>,
) -> Status {
    let tools = tools::available();
    let last = store::load_last(&items);
    let mut nav = Nav::new(items.len(), tools.len(), last.unwrap_or(0));
    let mut ui = Ui::new(gop, fonts, items, last, tools, store::secure_boot());

    // Wake on a key press or on a 100 ms timer instead of polling: no busy loop, and the
    // countdown does not drift with redraw time.
    // SAFETY: no notify function is registered, so there is no callback to misbehave.
    let timer = unsafe { boot::create_event(EventType::TIMER, Tpl::APPLICATION, None, None) };
    let key_event = system::with_stdin(|i| i.wait_for_key_event().ok());
    let (Ok(timer), Some(key_event)) = (timer, key_event) else {
        return text::menu(ui.items());
    };
    if boot::set_timer(&timer, TimerTrigger::Periodic(TICK)).is_err() {
        return text::menu(ui.items());
    }
    let events = [key_event, timer];

    // Only a known system may start by itself; media found through the generic path never does.
    let can_auto = |nav: &Nav, ui: &Ui| ui.items().get(nav.selected).is_some_and(|e| !e.manual_only);
    let mut left: Option<u32> = if can_auto(&nav, &ui) {
        Some(COUNTDOWN_TENTHS)
    } else {
        None
    };
    let mut message = String::new();

    ui.full(&nav, left, &message);

    loop {
        let key_ready = match boot::wait_for_event(&events) {
            Ok(i) => i == 0,
            Err(_) => {
                boot::stall(TICK);
                false
            }
        };
        let mut go = false;
        if key_ready {
            let key = system::with_stdin(|i| i.read_key()).ok().flatten();
            if let Some(k) = key {
                let old = nav.clone();
                left = None;
                message.clear();
                match k {
                    Key::Special(ScanCode::DOWN) => nav.press(Dir::Down),
                    Key::Special(ScanCode::UP) => nav.press(Dir::Up),
                    Key::Special(ScanCode::RIGHT) => nav.press(Dir::Right),
                    Key::Special(ScanCode::LEFT) => nav.press(Dir::Left),
                    Key::Printable(c) => {
                        let ch = char::from(c);
                        if ch == '\r' {
                            match nav.dock {
                                Some(d) => {
                                    if let Some(&t) = ui.tools().get(d) {
                                        message = tools::run(t);
                                    }
                                }
                                None => go = nav.n > 0,
                            }
                        } else if let Some(d) = ch.to_digit(10) {
                            nav.digit(d);
                        }
                    }
                    _ => {}
                }
                ui.update(&old, &nav, left, &message);
            }
        } else if let Some(t) = left {
            match countdown_step(t) {
                Some(rest) => {
                    left = Some(rest);
                    ui.tick(&nav, left, &message);
                }
                None => go = true,
            }
        }
        if go {
            ui.launching(&nav);
            let entry = &ui.items()[nav.selected];
            let name = entry.name;
            let outcome = entries::load(entry).and_then(|image| {
                // Remember it only once the firmware accepted the image.
                store::save_last(entry.id);
                entries::run(image)
            });
            // Reaching this line means the system did not take over.
            message = match outcome {
                Ok(()) => alloc::format!("{name} returned to the menu."),
                Err(e) => alloc::format!("Could not start {name}: {e}"),
            };
            left = None;
            ui.restore_mode();
            ui.full(&nav, left, &message);
        }
    }
}
