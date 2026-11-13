//! Painting. `Ui` owns the canvas and the graphics output; every method paints into the
//! canvas and then blits only the rectangle that changed.

use crate::entries::Entry;
use crate::gfx::*;
use crate::icons::{icon, tool_icon};
use crate::tools::Tool;
use alloc::vec::Vec;
use core::time::Duration;
use sail::layout::Layout;
use sail::nav::Nav;
use uefi::boot::{self, ScopedProtocol};
use uefi::proto::console::gop::{BltOp, BltPixel, BltRegion, GraphicsOutput};

pub const COUNTDOWN_TENTHS: u32 = 80;

type Sprite = Vec<BltPixel>;
type Rect = (usize, usize, usize, usize);

pub struct Ui {
    gop: ScopedProtocol<GraphicsOutput>,
    canvas: Canvas,
    fonts: Fonts,
    l: Layout,
    items: Vec<Entry>,
    /// Built on first use, so startup does not pay for tiles that are off screen.
    sprites: Vec<[Option<Sprite>; 2]>,
    last: Option<usize>,
    secure: Option<bool>,
    tools: Vec<Tool>,
}

impl Ui {
    pub fn new(
        gop: ScopedProtocol<GraphicsOutput>,
        fonts: Fonts,
        items: Vec<Entry>,
        last: Option<usize>,
        tools: Vec<Tool>,
        secure: Option<bool>,
    ) -> Self {
        let (w, h) = gop.current_mode_info().resolution();
        let l = Layout::new(w, h, items.len());
        let sprites = items.iter().map(|_| [None, None]).collect();
        Self {
            gop,
            canvas: Canvas::new(w, h),
            fonts,
            l,
            items,
            sprites,
            last,
            secure,
            tools,
        }
    }

    /// Re-apply the best mode after a chainloaded image returned and may have changed it.
    pub fn restore_mode(&mut self) {
        pick_best_mode(&mut self.gop);
    }

    pub fn items(&self) -> &[Entry] {
        &self.items
    }

    pub fn tools(&self) -> &[Tool] {
        &self.tools
    }

    fn blit(&mut self, (x, y, w, h): Rect) {
        let c = &self.canvas;
        let (x, y) = (x.min(c.w), y.min(c.h));
        let (w, h) = (w.min(c.w - x), h.min(c.h - y));
        if w == 0 || h == 0 {
            return;
        }
        // A failed blit only costs one frame; there is nothing useful to do about it.
        let _ = self.gop.blt(BltOp::BufferToVideo {
            buffer: &c.buf,
            src: BltRegion::SubRectangle {
                coords: (x, y),
                px_stride: c.w,
            },
            dest: (x, y),
            dims: (w, h),
        });
    }

    fn draw_tile(&self, e: usize, x: f32, y: f32, on: bool, canvas: &mut Canvas) {
        let f = &self.fonts;
        let tile = self.l.tile as f32;
        let rad = tile * 0.14;
        if on {
            canvas.rounded(x - 4.0, y - 4.0, tile + 8.0, tile + 8.0, rad + 4.0, ACCENT);
            canvas.rounded(x, y, tile, tile, rad, PANEL);
        } else {
            canvas.rounded(x, y, tile, tile, rad, LINE);
            canvas.rounded(x + 1.5, y + 1.5, tile - 3.0, tile - 3.0, rad, BG);
        }
        let entry = &self.items[e];
        let cx = x + tile / 2.0;
        icon(canvas, f, entry.kind, cx, y + tile * 0.40, tile * 0.42);
        let npx = (tile / 9.0).max(14.0);
        canvas.centered(
            &f.medium,
            entry.name,
            cx,
            y + tile * 0.68,
            npx,
            if on { 0xffffff } else { TEXT },
        );
        canvas.centered(
            &f.regular,
            entry.note,
            cx,
            y + tile * 0.68 + npx * 1.35,
            npx * 0.72,
            MUTED,
        );
        if self.last == Some(e) {
            let px = (tile / 13.0).max(11.0);
            let tw = f.medium.measure("Last used", px);
            let icon_w = px * 1.5;
            let (pw, ph) = (tw + icon_w + px * 1.9, px * 2.0);
            let (bx, by) = (cx - pw / 2.0, y - ph / 2.0);
            let thick = (px * 0.12).max(1.4);
            canvas.rounded(bx - 3.0, by - 3.0, pw + 6.0, ph + 6.0, ph / 2.0 + 3.0, BG);
            canvas.rounded(bx, by, pw, ph, ph / 2.0, ACCENT);
            let (ix, iy, ir) = (bx + px + icon_w / 2.0, by + ph / 2.0, px * 0.5);
            canvas.ring(ix, iy, ir, thick, 0xffffff);
            canvas.line(ix, iy, ix, iy - ir * 0.62, thick, 0xffffff);
            canvas.line(ix, iy, ix + ir * 0.5, iy, thick, 0xffffff);
            canvas.text(
                &f.medium,
                "Last used",
                bx + px * 1.1 + icon_w,
                by + (ph - px) / 2.0 - px * 0.12,
                px,
                0xffffff,
            );
        }
    }

    fn ensure_sprite(&mut self, i: usize, on: bool) {
        let k = on as usize;
        if self.sprites[i][k].is_some() {
            return;
        }
        let mut c = Canvas::new(self.l.side, self.l.side + self.l.pad);
        self.draw_tile(i, 4.0, 4.0 + self.l.pad as f32, on, &mut c);
        self.sprites[i][k] = Some(c.buf);
    }

    /// `selected` is `usize::MAX` while the tool row has focus (no tile highlighted).
    fn paint_strip(&mut self, selected: usize, scroll_px: f32) {
        let l = self.l.clone();
        let (sx, sy, sw, sh) = l.strip_rect();
        self.canvas.rect(sx, sy, sw, sh, BG);
        let (cl, cr) = (sx as i32, (sx + sw) as i32);
        for i in 0..self.items.len() {
            let x = l.tile_x(i, scroll_px);
            if x + l.side as i32 <= cl || x >= cr {
                continue;
            }
            let on = i == selected;
            self.ensure_sprite(i, on);
            let Some(img) = self.sprites[i][on as usize].as_ref() else {
                continue;
            };
            let from = (cl - x).max(0) as usize;
            let to = ((cr - x) as usize).min(l.side);
            for row in 0..l.side + l.pad {
                let dst = (sy + row) * self.canvas.w + (x + from as i32) as usize;
                self.canvas.buf[dst..dst + (to - from)].copy_from_slice(&img[row * l.side + from..row * l.side + to]);
            }
        }
    }

    fn paint_arrows_dots(&mut self, nav: &Nav) -> [Rect; 3] {
        let l = self.l.clone();
        let c = &mut self.canvas;
        let n = nav.n;
        let ar = l.h as f32 / 40.0;
        let mid = l.y0 as f32 + l.tile as f32 / 2.0;
        let aw = (l.gap as f32 * 3.4) as usize;
        let ah = (ar * 3.0) as usize;
        let ay = (mid - ar * 1.5) as usize;
        let lx = l.x0.saturating_sub(aw + 4);
        let rx = l.x0 + l.row_w + 4;
        c.rect(lx, ay, aw, ah, BG);
        c.rect(rx, ay, aw, ah, BG);
        if nav.offset > 0 {
            let x = l.x0 as f32 - l.gap as f32 * 1.8;
            c.line(x + ar * 0.4, mid - ar, x - ar * 0.6, mid, 4.0, MUTED);
            c.line(x - ar * 0.6, mid, x + ar * 0.4, mid + ar, 4.0, MUTED);
        }
        if nav.offset + l.shown < n {
            let x = (l.x0 + l.row_w) as f32 + l.gap as f32 * 1.8;
            c.line(x - ar * 0.4, mid - ar, x + ar * 0.6, mid, 4.0, MUTED);
            c.line(x + ar * 0.6, mid, x - ar * 0.4, mid + ar, 4.0, MUTED);
        }
        let dots_y = (l.y0 + l.tile) as f32 + l.gap as f32 * 1.5;
        let step = l.h as f32 / 40.0;
        let dw = (step * n as f32 + step * 2.0) as usize;
        let dx = (l.w / 2).saturating_sub(dw / 2);
        let dy = (dots_y - step) as usize;
        c.rect(dx, dy, dw, (step * 2.0) as usize, BG);
        if n > l.shown {
            let x0 = l.w as f32 / 2.0 - step * (n as f32 - 1.0) / 2.0;
            for i in 0..n {
                let on = i == nav.selected;
                c.circle(
                    x0 + i as f32 * step,
                    dots_y,
                    if on { step * 0.22 } else { step * 0.15 },
                    if on { ACCENT } else { LINE },
                );
            }
        }
        [(lx, ay, aw, ah), (rx, ay, aw, ah), (dx, dy, dw, (step * 2.0) as usize)]
    }

    fn paint_dock(&mut self, focus: Option<usize>) -> Rect {
        let (y, h) = self.l.dock_geometry();
        let (rx, rw) = ((self.l.w as f32 * 0.15) as usize, (self.l.w as f32 * 0.7) as usize);
        let (f, c) = (&self.fonts, &mut self.canvas);
        c.rect(rx, (y - 4.0) as usize, rw, (h + 8.0) as usize, BG);
        let px = h * 0.42;
        let pad = px * 1.1;
        let gap = h * 0.35;
        let (iw, ig) = (px * 1.25, px * 0.6);
        let widths: Vec<f32> = self
            .tools
            .iter()
            .map(|t| f.regular.measure(t.label(), px) + pad * 2.0 + iw + ig)
            .collect();
        let total: f32 = widths.iter().sum::<f32>() + gap * (self.tools.len() as f32 - 1.0);
        let mut x = (self.l.w as f32 - total) / 2.0;
        for (i, t) in self.tools.iter().enumerate() {
            let on = focus == Some(i);
            if on {
                c.rounded(x - 2.0, y - 2.0, widths[i] + 4.0, h + 4.0, h / 2.0 + 2.0, ACCENT);
                c.rounded(x, y, widths[i], h, h / 2.0, PANEL);
            } else {
                c.rounded(x, y, widths[i], h, h / 2.0, LINE);
                c.rounded(x + 1.5, y + 1.5, widths[i] - 3.0, h - 3.0, h / 2.0 - 1.5, BG);
            }
            let col = if on { 0xffffff } else { MUTED };
            tool_icon(c, t.icon_index(), x + pad + iw / 2.0, y + h / 2.0, iw / 2.0, col);
            c.text(
                &f.regular,
                t.label(),
                x + pad + iw + ig,
                y + (h - px) / 2.0 - px * 0.12,
                px,
                col,
            );
            x += widths[i] + gap;
        }
        (rx, (y - 4.0) as usize, rw, (h + 8.0) as usize)
    }

    fn paint_footer(&mut self, nav: &Nav, left: Option<u32>, message: &str) -> Rect {
        let l = self.l.clone();
        let (f, c) = (&self.fonts, &mut self.canvas);
        let w = l.w as f32;
        let (rx, ry, rw, rh) = l.footer_rect();
        c.rect(rx, ry, rw, rh, BG);
        if !message.is_empty() {
            c.centered(&f.medium, message, w / 2.0, l.fy - l.fpx * 2.6, l.fpx, ERROR);
        }
        let hint = if self.items.len() > 1 {
            "Left Right select    Enter start    Down tools"
        } else {
            "Enter start    Down tools"
        };
        c.centered(&f.regular, hint, w / 2.0, l.fy, l.fpx, MUTED);
        if let (Some(t), Some(e)) = (left, self.items.get(nav.selected)) {
            let secs = t.div_ceil(10);
            let s = alloc::format!("Starting {} in {}", e.name, secs);
            c.centered(&f.regular, &s, w / 2.0, l.fy + l.fpx * 1.5, l.fpx, MUTED);
            let bw = (w * 0.3) as usize;
            let by = (l.fy + l.fpx * 3.2) as usize;
            c.rect((l.w - bw) / 2, by, bw, 3, LINE);
            c.rect(
                (l.w - bw) / 2,
                by,
                bw * t as usize / COUNTDOWN_TENTHS as usize,
                3,
                ACCENT,
            );
        }
        (rx, ry, rw, rh)
    }

    fn paint_header(&mut self) {
        let (w, h) = (self.canvas.w as f32, self.canvas.h as f32);
        let (f, c) = (&self.fonts, &mut self.canvas);
        c.centered(&f.bold, "Sail", w / 2.0, h * 0.06, h / 13.0, TEXT);
        c.centered(
            &f.regular,
            "Choose a system",
            w / 2.0,
            h * 0.06 + h / 9.0,
            h / 34.0,
            MUTED,
        );
        let (text, color) = match self.secure {
            Some(true) => ("Secure Boot on", OK),
            Some(false) => ("Secure Boot off", MUTED),
            None => return,
        };
        c.centered(
            &f.regular,
            text,
            w / 2.0,
            h * 0.06 + h / 9.0 + h / 34.0 * 1.6,
            h / 52.0,
            color,
        );
    }

    /// Paint everything and push the whole frame.
    pub fn full(&mut self, nav: &Nav, left: Option<u32>, message: &str) {
        self.canvas.clear();
        self.paint_header();
        if self.items.is_empty() {
            let (w, h) = (self.canvas.w as f32, self.canvas.h as f32);
            let f = &self.fonts;
            self.canvas
                .centered(&f.medium, "No systems found", w / 2.0, h * 0.45, h / 24.0, TEXT);
            self.canvas.centered(
                &f.regular,
                "Add a boot loader to the EFI partition",
                w / 2.0,
                h * 0.45 + h / 18.0,
                h / 38.0,
                MUTED,
            );
            self.paint_dock(nav.dock);
        } else {
            let sel = if nav.dock.is_some() { usize::MAX } else { nav.selected };
            self.paint_strip(sel, (nav.offset * self.l.step) as f32);
            self.paint_arrows_dots(nav);
            self.paint_dock(nav.dock);
            self.paint_footer(nav, left, message);
        }
        let (w, h) = (self.canvas.w, self.canvas.h);
        self.blit((0, 0, w, h));
    }

    /// Repaint after a key press changed `old` into `nav`.
    pub fn update(&mut self, old: &Nav, nav: &Nav, left: Option<u32>, message: &str) {
        if nav.n > 0 {
            let sel = if nav.dock.is_some() { usize::MAX } else { nav.selected };
            let l = self.l.clone();
            let (sx, sy, sw, sh) = l.strip_rect();
            if nav.offset != old.offset {
                let (from, to) = ((old.offset * l.step) as f32, (nav.offset * l.step) as f32);
                const FRAMES: u32 = 9;
                for fr in 1..=FRAMES {
                    let t = fr as f32 / FRAMES as f32;
                    let e = 1.0 - (1.0 - t) * (1.0 - t) * (1.0 - t);
                    self.paint_strip(sel, from + (to - from) * e);
                    self.blit((sx, sy, sw, sh));
                    boot::stall(Duration::from_millis(8));
                }
            } else if nav.dock != old.dock {
                self.paint_strip(sel, (nav.offset * l.step) as f32);
                self.blit((sx, sy, sw, sh));
            } else if nav.selected != old.selected {
                self.paint_strip(sel, (nav.offset * l.step) as f32);
                for i in [old.selected, nav.selected] {
                    let x = l.tile_x(i, (nav.offset * l.step) as f32).max(sx as i32) as usize;
                    self.blit((x, sy, l.side, sh));
                }
            }
            for r in self.paint_arrows_dots(nav) {
                self.blit(r);
            }
        }
        let r = self.paint_dock(nav.dock);
        self.blit(r);
        let r = self.paint_footer(nav, left, message);
        self.blit(r);
    }

    /// Countdown tick: only the footer changes.
    pub fn tick(&mut self, nav: &Nav, left: Option<u32>, message: &str) {
        let r = self.paint_footer(nav, left, message);
        self.blit(r);
    }

    pub fn launching(&mut self, nav: &Nav) {
        let l = self.l.clone();
        let scroll = (nav.offset * l.step) as f32;
        self.paint_strip(nav.selected, scroll);
        let (sx, sy, sw, sh) = l.strip_rect();
        for i in 0..self.items.len() {
            if i == nav.selected {
                continue;
            }
            let x = l.tile_x(i, scroll);
            let x0 = x.max(sx as i32) as usize;
            let x1 = (x + l.side as i32).min((sx + sw) as i32);
            if x1 > x0 as i32 {
                self.canvas.dim(x0, sy, (x1 - x0 as i32) as usize, sh, 0.7);
            }
        }
        let (rx, ry, rw, rh) = l.footer_rect();
        self.canvas.rect(rx, ry, rw, rh, BG);
        let msg = alloc::format!("Starting {}...", self.items[nav.selected].name);
        let f = &self.fonts;
        self.canvas
            .centered(&f.medium, &msg, l.w as f32 / 2.0, l.fy, l.fpx * 1.25, TEXT);
        let bw = (l.w as f32 * 0.3) as usize;
        self.canvas
            .rect((l.w - bw) / 2, (l.fy + l.fpx * 2.4) as usize, bw, 3, ACCENT);
        self.blit((sx, sy, sw, sh));
        self.blit((rx, ry, rw, rh));
    }
}

/// The largest mode up to 1920x1200: more pixels only make the software renderer slower.
pub fn pick_best_mode(gop: &mut GraphicsOutput) {
    let best = gop
        .modes()
        .filter(|m| {
            let (mw, mh) = m.info().resolution();
            mw <= 1920 && mh <= 1200
        })
        .max_by_key(|m| {
            let (mw, mh) = m.info().resolution();
            mw * mh
        });
    if let Some(m) = best
        && m.info().resolution() != gop.current_mode_info().resolution()
    {
        let _ = gop.set_mode(&m);
    }
}
