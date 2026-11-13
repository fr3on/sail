//! Fallback when there is no graphics output (or the fonts cannot load):
//! a plain numbered list on the text console. Esc hands control back to the firmware,
//! which then tries the next boot option.

use crate::entries::{self, Entry};
use crate::store;
use core::fmt::Write;
use uefi::boot;
use uefi::prelude::*;
use uefi::proto::console::text::{Key, ScanCode};
use uefi::system;

pub fn menu(items: &[Entry]) -> Status {
    if items.is_empty() {
        return Status::NOT_FOUND;
    }
    let Some(key_event) = system::with_stdin(|i| i.wait_for_key_event().ok()) else {
        return Status::UNSUPPORTED;
    };
    let events = [key_event];
    let mut note = "";
    loop {
        system::with_stdout(|o| {
            let _ = o.clear();
            let _ = writeln!(o, "Sail - choose a system\n");
            for (i, e) in items.iter().enumerate().take(9) {
                let _ = writeln!(o, "  {}  {} ({})", i + 1, e.name, e.note);
            }
            let _ = writeln!(o, "\nPress a number to start. Esc leaves Sail.\n{note}");
        });
        note = "";
        if boot::wait_for_event(&events).is_err() {
            return Status::DEVICE_ERROR;
        }
        match system::with_stdin(|i| i.read_key()) {
            Ok(Some(Key::Special(ScanCode::ESCAPE))) => return Status::ABORTED,
            Ok(Some(Key::Printable(c))) => {
                let Some(d) = char::from(c).to_digit(10) else { continue };
                let Some(e) = items.get((d as usize).wrapping_sub(1)).filter(|_| d >= 1) else {
                    continue;
                };
                match entries::load(e) {
                    Ok(image) => {
                        store::save_last(e.id);
                        if entries::run(image).is_err() {
                            note = "That system returned an error.";
                        }
                    }
                    Err(_) => note = "Could not start that system.",
                }
            }
            _ => {}
        }
    }
}
