#![no_main]
#![no_std]

extern crate alloc;

use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use core::time::Duration;
use fontdue::{Font, FontSettings};
use uefi::prelude::*;
use uefi::proto::console::gop::{BltOp, BltPixel, BltRegion, GraphicsOutput};
use uefi::proto::console::text::{Key, ScanCode};
use uefi::proto::device_path::build::{media::FilePath, DevicePathBuilder};
use uefi::proto::device_path::DevicePath;
use uefi::proto::loaded_image::LoadedImage;
use uefi::proto::media::fs::SimpleFileSystem;
use uefi::boot::{OpenProtocolAttributes, OpenProtocolParams, SearchType};
use uefi::{boot, cstr16, system, CStr16, Identify};

#[used]
#[unsafe(link_section = ".sbat")]
static SBAT: [u8; 130] = *b"sbat,1,SBAT Version,sbat,1,https://github.com/rhboot/shim/blob/main/SBAT.md\nsail,1,Sail,sail,0.1.0,https://github.com/fr3on/sail\n\0";

const BG: u32 = 0x0d1117;
const PANEL: u32 = 0x161d2b;
const LINE: u32 = 0x2a313b;
const TEXT: u32 = 0xf0f3f6;
const MUTED: u32 = 0x8b949e;
const ACCENT: u32 = 0x3b82f6;
const ERROR: u32 = 0xf85149;

const COUNTDOWN_TENTHS: u32 = 80;
const MAX_TILES: usize = 32;
const VISIBLE: usize = 4;

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Fedora,
    Windows,
    Ubuntu,
    Debian,
    Arch,
    Suse,
    Mint,
    Other,
}

struct Known {
    kind: Kind,
    name: &'static str,
    note: &'static str,
    path: &'static CStr16,
}

const KNOWN: &[Known] = &[
    Known { kind: Kind::Fedora, name: "Fedora", note: "Linux", path: cstr16!("\\EFI\\fedora\\shimx64.efi") },
    Known { kind: Kind::Ubuntu, name: "Ubuntu", note: "Linux", path: cstr16!("\\EFI\\ubuntu\\shimx64.efi") },
    Known { kind: Kind::Debian, name: "Debian", note: "Linux", path: cstr16!("\\EFI\\debian\\shimx64.efi") },
    Known { kind: Kind::Arch, name: "Arch Linux", note: "Linux", path: cstr16!("\\EFI\\arch\\grubx64.efi") },
    Known { kind: Kind::Suse, name: "openSUSE", note: "Linux", path: cstr16!("\\EFI\\opensuse\\grubx64.efi") },
    Known { kind: Kind::Mint, name: "Linux Mint", note: "Linux", path: cstr16!("\\EFI\\linuxmint\\shimx64.efi") },
    Known { kind: Kind::Windows, name: "Windows 11", note: "Windows", path: cstr16!("\\EFI\\Microsoft\\Boot\\bootmgfw.efi") },
];

struct Entry {
    kind: Kind,
    name: &'static str,
    note: &'static str,
    path: &'static CStr16,
    device: Handle,
}

struct Fonts {
    regular: Font,
    medium: Font,
    bold: Font,
}

fn load_fonts() -> Fonts {
    let f = |b: &[u8]| Font::from_bytes(b, FontSettings::default()).unwrap();
    Fonts {
        regular: f(include_bytes!("../assets/DMSans-Regular.ttf")),
        medium: f(include_bytes!("../assets/DMSans-Medium.ttf")),
        bold: f(include_bytes!("../assets/DMSans-Bold.ttf")),
    }
}

fn discover() -> Vec<Entry> {
    let mut out = Vec::new();
    let me = boot::image_handle();
    let own_device = boot::open_protocol_exclusive::<LoadedImage>(me).ok().and_then(|li| li.device());
    let Ok(handles) = boot::locate_handle_buffer(SearchType::ByProtocol(&SimpleFileSystem::GUID)) else { return out };
    for &h in handles.iter() {
        let proto = unsafe {
            boot::open_protocol::<SimpleFileSystem>(
                OpenProtocolParams { handle: h, agent: me, controller: None },
                OpenProtocolAttributes::GetProtocol,
            )
        };
        let Ok(proto) = proto else { continue };
        let mut fs = uefi::fs::FileSystem::new(proto);
        for k in KNOWN {
            if out.len() >= MAX_TILES {
                break;
            }
            if fs.try_exists(uefi::fs::Path::new(k.path)).unwrap_or(false) {
                out.push(Entry { kind: k.kind, name: k.name, note: k.note, path: k.path, device: h });
            }
        }
        let fallback = cstr16!("\\EFI\\BOOT\\BOOTX64.EFI");
        if Some(h) != own_device && out.len() < MAX_TILES && fs.try_exists(uefi::fs::Path::new(fallback)).unwrap_or(false) {
            out.push(Entry { kind: Kind::Other, name: "Other disk", note: "USB or drive", path: fallback, device: h });
        }
    }
    out.sort_by_key(|e| match e.kind { Kind::Other => 2, Kind::Windows => 1, _ => 0 });
    out
}

struct Canvas {
    w: usize,
    h: usize,
    buf: Vec<BltPixel>,
}

fn mix(a: u32, b: u32, t: f32) -> u32 {
    let ch = |s: u32| {
        let (x, y) = (((a >> s) & 255) as f32, ((b >> s) & 255) as f32);
        ((x + (y - x) * t) as u32) & 255
    };
    (ch(16) << 16) | (ch(8) << 8) | ch(0)
}

impl Canvas {
    fn new(w: usize, h: usize) -> Self {
        Self { w, h, buf: vec![BltPixel::from(BG); w * h] }
    }

    fn clear(&mut self) {
        self.buf.fill(BltPixel::from(BG));
    }

    fn put(&mut self, x: i32, y: i32, color: u32, alpha: f32) {
        if x < 0 || y < 0 || x as usize >= self.w || y as usize >= self.h || alpha <= 0.0 {
            return;
        }
        let i = y as usize * self.w + x as usize;
        let p = self.buf[i];
        let under = ((p.red as u32) << 16) | ((p.green as u32) << 8) | p.blue as u32;
        self.buf[i] = BltPixel::from(mix(under, color, alpha.min(1.0)));
    }

    fn dim(&mut self, x: usize, y: usize, w: usize, h: usize, t: f32) {
        for row in y..(y + h).min(self.h) {
            for col in x..(x + w).min(self.w) {
                let p = self.buf[row * self.w + col];
                let c = ((p.red as u32) << 16) | ((p.green as u32) << 8) | p.blue as u32;
                self.buf[row * self.w + col] = BltPixel::from(mix(c, BG, t));
            }
        }
    }

    fn rect(&mut self, x: usize, y: usize, w: usize, h: usize, color: u32) {
        let c = BltPixel::from(color);
        for row in y..(y + h).min(self.h) {
            let start = row * self.w + x.min(self.w);
            let end = row * self.w + (x + w).min(self.w);
            self.buf[start..end].fill(c);
        }
    }

    fn rounded(&mut self, x: f32, y: f32, w: f32, h: f32, r: f32, color: u32) {
        let (cx, cy) = (x + w / 2.0, y + h / 2.0);
        let (hx, hy) = (w / 2.0 - r, h / 2.0 - r);
        for py in (y as i32 - 1)..(y + h) as i32 + 2 {
            for px in (x as i32 - 1)..(x + w) as i32 + 2 {
                let dx = (libm::fabsf(px as f32 + 0.5 - cx) - hx).max(0.0);
                let dy = (libm::fabsf(py as f32 + 0.5 - cy) - hy).max(0.0);
                let d = libm::sqrtf(dx * dx + dy * dy) - r;
                self.put(px, py, color, 0.5 - d);
            }
        }
    }

    fn circle(&mut self, cx: f32, cy: f32, r: f32, color: u32) {
        self.rounded(cx - r, cy - r, r * 2.0, r * 2.0, r, color);
    }

    #[allow(dead_code)]
    fn ring(&mut self, cx: f32, cy: f32, r: f32, thick: f32, color: u32) {
        for py in (cy - r) as i32 - 2..(cy + r) as i32 + 3 {
            for px in (cx - r) as i32 - 2..(cx + r) as i32 + 3 {
                let (dx, dy) = (px as f32 + 0.5 - cx, py as f32 + 0.5 - cy);
                let d = libm::fabsf(libm::sqrtf(dx * dx + dy * dy) - r) - thick / 2.0;
                self.put(px, py, color, 0.5 - d);
            }
        }
    }

    fn line(&mut self, x0: f32, y0: f32, x1: f32, y1: f32, thick: f32, color: u32) {
        let (minx, maxx) = (x0.min(x1) - thick - 2.0, x0.max(x1) + thick + 2.0);
        let (miny, maxy) = (y0.min(y1) - thick - 2.0, y0.max(y1) + thick + 2.0);
        let (dx, dy) = (x1 - x0, y1 - y0);
        let len2 = dx * dx + dy * dy;
        for py in miny as i32..maxy as i32 {
            for px in minx as i32..maxx as i32 {
                let (qx, qy) = (px as f32 + 0.5 - x0, py as f32 + 0.5 - y0);
                let t = ((qx * dx + qy * dy) / len2).clamp(0.0, 1.0);
                let (ex, ey) = (qx - t * dx, qy - t * dy);
                let d = libm::sqrtf(ex * ex + ey * ey) - thick / 2.0;
                self.put(px, py, color, 0.5 - d);
            }
        }
    }

    fn measure(font: &Font, text: &str, px: f32) -> f32 {
        text.chars().map(|c| font.metrics(c, px).advance_width).sum()
    }

    fn text(&mut self, font: &Font, text: &str, x: f32, top: f32, px: f32, color: u32) {
        let ascent = font.horizontal_line_metrics(px).map(|m| m.ascent).unwrap_or(px * 0.8);
        let baseline = top + ascent;
        let mut pen = x;
        for ch in text.chars() {
            let (m, bitmap) = font.rasterize(ch, px);
            let gx = pen as i32 + m.xmin;
            let gy = (baseline - m.height as f32 - m.ymin as f32) as i32;
            for row in 0..m.height {
                for col in 0..m.width {
                    let a = bitmap[row * m.width + col] as f32 / 255.0;
                    self.put(gx + col as i32, gy + row as i32, color, a);
                }
            }
            pen += m.advance_width;
        }
    }

    fn centered(&mut self, font: &Font, text: &str, cx: f32, top: f32, px: f32, color: u32) {
        let w = Self::measure(font, text, px);
        self.text(font, text, cx - w / 2.0, top, px, color);
    }
}

const MASK: usize = 192;
static FEDORA: &[u8] = include_bytes!("../assets/icons/fedora.a");
static UBUNTU: &[u8] = include_bytes!("../assets/icons/ubuntu.a");
static DEBIAN: &[u8] = include_bytes!("../assets/icons/debian.a");
static ARCH: &[u8] = include_bytes!("../assets/icons/archlinux.a");
static SUSE: &[u8] = include_bytes!("../assets/icons/opensuse.a");
static MINT: &[u8] = include_bytes!("../assets/icons/linuxmint.a");

fn letter(c: &mut Canvas, f: &Fonts, ch: &str, cx: f32, cy: f32, size: f32) {
    let px = size * 0.62;
    let w = Canvas::measure(&f.bold, ch, px);
    c.text(&f.bold, ch, cx - w / 2.0, cy - px * 0.52, px, 0xffffff);
}

fn mask(c: &mut Canvas, data: &[u8], cx: f32, cy: f32, size: f32, color: u32) {
    let (x0, y0) = (cx - size / 2.0, cy - size / 2.0);
    let at = |x: i32, y: i32| data[y.clamp(0, MASK as i32 - 1) as usize * MASK + x.clamp(0, MASK as i32 - 1) as usize] as f32 / 255.0;
    for py in y0 as i32..(y0 + size) as i32 + 1 {
        for px in x0 as i32..(x0 + size) as i32 + 1 {
            let u = (px as f32 + 0.5 - x0) / size * MASK as f32 - 0.5;
            let v = (py as f32 + 0.5 - y0) / size * MASK as f32 - 0.5;
            let (iu, iv) = (libm::floorf(u), libm::floorf(v));
            let (fu, fv) = (u - iu, v - iv);
            let (iu, iv) = (iu as i32, iv as i32);
            let top = at(iu, iv) * (1.0 - fu) + at(iu + 1, iv) * fu;
            let bot = at(iu, iv + 1) * (1.0 - fu) + at(iu + 1, iv + 1) * fu;
            c.put(px, py, color, top * (1.0 - fv) + bot * fv);
        }
    }
}

fn icon(c: &mut Canvas, f: &Fonts, kind: Kind, cx: f32, cy: f32, s: f32) {
    match kind {
        Kind::Windows => {
            let g = s * 0.05;
            let q = (s * 0.86 - g) / 2.0;
            let (x0, y0) = (cx - q - g / 2.0, cy - q - g / 2.0);
            for (i, j) in [(0.0, 0.0), (1.0, 0.0), (0.0, 1.0), (1.0, 1.0)] {
                c.rounded(x0 + i * (q + g), y0 + j * (q + g), q, q, s * 0.02, 0x0a8ee8);
            }
        }
        Kind::Fedora => mask(c, FEDORA, cx, cy, s * 1.08, 0x51a2da),
        Kind::Ubuntu => mask(c, UBUNTU, cx, cy, s * 1.08, 0xe95420),
        Kind::Debian => mask(c, DEBIAN, cx, cy, s * 1.08, 0xd70751),
        Kind::Arch => mask(c, ARCH, cx, cy, s * 1.08, 0x1793d1),
        Kind::Suse => mask(c, SUSE, cx, cy, s * 1.08, 0x73ba25),
        Kind::Mint => mask(c, MINT, cx, cy, s * 1.08, 0x87cf3e),
        Kind::Other => {
            c.circle(cx, cy, s / 2.0, 0x4b5563);
            letter(c, f, "?", cx, cy, s);
        }
    }
}

struct Layout {
    w: usize,
    h: usize,
    tile: usize,
    gap: usize,
    shown: usize,
    x0: usize,
    y0: usize,
    row_w: usize,
    side: usize,
    step: usize,
    fpx: f32,
    fy: f32,
}

fn make_layout(w: usize, h: usize, n: usize) -> Layout {
    let shown = n.clamp(1, VISIBLE);
    let gap = h / 28;
    let avail_w = (w as f32 * 0.80) as usize;
    let tile = (((avail_w - gap * (shown - 1)) / shown).min(h * 36 / 100)).max(64);
    let row_w = tile * shown + gap * (shown - 1);
    Layout {
        w,
        h,
        tile,
        gap,
        shown,
        x0: (w - row_w) / 2,
        y0: h * 30 / 100 + (h * 40 / 100).saturating_sub(tile) / 2,
        row_w,
        side: tile + 8,
        step: tile + gap,
        fpx: h as f32 / 38.0,
        fy: h as f32 * 0.90,
    }
}

fn draw_tile(c: &mut Canvas, f: &Fonts, e: &Entry, x: f32, y: f32, tile: f32, on: bool) {
    let rad = tile * 0.14;
    if on {
        c.rounded(x - 4.0, y - 4.0, tile + 8.0, tile + 8.0, rad + 4.0, ACCENT);
        c.rounded(x, y, tile, tile, rad, PANEL);
    } else {
        c.rounded(x, y, tile, tile, rad, LINE);
        c.rounded(x + 1.5, y + 1.5, tile - 3.0, tile - 3.0, rad, BG);
    }
    let cx = x + tile / 2.0;
    icon(c, f, e.kind, cx, y + tile * 0.40, tile * 0.42);
    let npx = (tile / 9.0).max(14.0);
    c.centered(&f.medium, e.name, cx, y + tile * 0.68, npx, if on { 0xffffff } else { TEXT });
    c.centered(&f.regular, e.note, cx, y + tile * 0.68 + npx * 1.35, npx * 0.72, MUTED);
}

fn make_sprites(f: &Fonts, items: &[Entry], l: &Layout) -> Vec<[Vec<BltPixel>; 2]> {
    items
        .iter()
        .map(|e| {
            let mut out = [Vec::new(), Vec::new()];
            for (k, on) in [false, true].into_iter().enumerate() {
                let mut c = Canvas::new(l.side, l.side);
                draw_tile(&mut c, f, e, 4.0, 4.0, l.tile as f32, on);
                out[k] = c.buf;
            }
            out
        })
        .collect()
}

fn blit_rect(gop: &mut GraphicsOutput, c: &Canvas, x: usize, y: usize, w: usize, h: usize) {
    let (x, y) = (x.min(c.w), y.min(c.h));
    let (w, h) = (w.min(c.w - x), h.min(c.h - y));
    if w == 0 || h == 0 {
        return;
    }
    let _ = gop.blt(BltOp::BufferToVideo {
        buffer: &c.buf,
        src: BltRegion::SubRectangle { coords: (x, y), px_stride: c.w },
        dest: (x, y),
        dims: (w, h),
    });
}

fn clear_rect(c: &mut Canvas, x: usize, y: usize, w: usize, h: usize) {
    c.rect(x, y, w, h, BG);
}

fn strip_rect(l: &Layout) -> (usize, usize, usize, usize) {
    (l.x0 - 4, l.y0 - 4, l.row_w + 8, l.side)
}

fn tile_x(l: &Layout, i: usize, scroll_px: f32) -> i32 {
    (l.x0 as f32 + i as f32 * l.step as f32 - scroll_px) as i32 - 4
}

fn paint_strip(c: &mut Canvas, l: &Layout, sprites: &[[Vec<BltPixel>; 2]], selected: usize, scroll_px: f32) {
    let (sx, sy, sw, sh) = strip_rect(l);
    clear_rect(c, sx, sy, sw, sh);
    let (cl, cr) = (sx as i32, (sx + sw) as i32);
    for (i, sp) in sprites.iter().enumerate() {
        let x = tile_x(l, i, scroll_px);
        if x + l.side as i32 <= cl || x >= cr {
            continue;
        }
        let img = &sp[(i == selected) as usize];
        let from = (cl - x).max(0) as usize;
        let to = ((cr - x) as usize).min(l.side);
        for row in 0..l.side {
            let dst = (sy + row) * c.w + (x + from as i32) as usize;
            c.buf[dst..dst + (to - from)].copy_from_slice(&img[row * l.side + from..row * l.side + to]);
        }
    }
}

fn paint_arrows_dots(c: &mut Canvas, l: &Layout, n: usize, offset: usize, selected: usize) -> [(usize, usize, usize, usize); 3] {
    let ar = l.h as f32 / 40.0;
    let mid = l.y0 as f32 + l.tile as f32 / 2.0;
    let aw = (l.gap as f32 * 3.4) as usize;
    let ah = (ar * 3.0) as usize;
    let ay = (mid - ar * 1.5) as usize;
    let lx = l.x0.saturating_sub(aw + 4);
    let rx = l.x0 + l.row_w + 4;
    clear_rect(c, lx, ay, aw, ah);
    clear_rect(c, rx, ay, aw, ah);
    if offset > 0 {
        let x = l.x0 as f32 - l.gap as f32 * 1.8;
        c.line(x + ar * 0.4, mid - ar, x - ar * 0.6, mid, 4.0, MUTED);
        c.line(x - ar * 0.6, mid, x + ar * 0.4, mid + ar, 4.0, MUTED);
    }
    if offset + l.shown < n {
        let x = (l.x0 + l.row_w) as f32 + l.gap as f32 * 1.8;
        c.line(x - ar * 0.4, mid - ar, x + ar * 0.6, mid, 4.0, MUTED);
        c.line(x + ar * 0.6, mid, x - ar * 0.4, mid + ar, 4.0, MUTED);
    }
    let dots_y = (l.y0 + l.tile) as f32 + l.gap as f32 * 1.5;
    let step = l.h as f32 / 40.0;
    let dw = (step * n as f32 + step * 2.0) as usize;
    let dx = (l.w / 2).saturating_sub(dw / 2);
    let dy = (dots_y - step) as usize;
    clear_rect(c, dx, dy, dw, (step * 2.0) as usize);
    if n > l.shown {
        let x0 = l.w as f32 / 2.0 - step * (n as f32 - 1.0) / 2.0;
        for i in 0..n {
            let on = i == selected;
            c.circle(x0 + i as f32 * step, dots_y, if on { step * 0.22 } else { step * 0.15 }, if on { ACCENT } else { LINE });
        }
    }
    [(lx, ay, aw, ah), (rx, ay, aw, ah), (dx, dy, dw, (step * 2.0) as usize)]
}

fn paint_footer(c: &mut Canvas, f: &Fonts, l: &Layout, items: &[Entry], selected: usize, left: Option<u32>, message: &str) -> (usize, usize, usize, usize) {
    let w = l.w as f32;
    let top = (l.y0 + l.tile) as f32 + l.gap as f32 * 2.6;
    let (rx, ry, rw) = ((l.w as f32 * 0.15) as usize, top as usize, (l.w as f32 * 0.7) as usize);
    let rh = (l.fy + l.fpx * 3.8 - top) as usize;
    clear_rect(c, rx, ry, rw, rh);
    if !message.is_empty() {
        c.centered(&f.medium, message, w / 2.0, top, l.fpx, ERROR);
    }
    let hint = if items.len() > 1 { "Left Right select    Enter start" } else { "Enter start" };
    c.centered(&f.regular, hint, w / 2.0, l.fy, l.fpx, MUTED);
    if let Some(t) = left {
        if let Some(e) = items.get(selected) {
            let secs = (t + 9) / 10;
            let mut s = String::from("Starting ");
            s.push_str(e.name);
            s.push_str(" in ");
            s.push(char::from(b'0' + (secs % 10) as u8));
            c.centered(&f.regular, &s, w / 2.0, l.fy + l.fpx * 1.5, l.fpx, MUTED);
            let bw = (w * 0.3) as usize;
            let by = (l.fy + l.fpx * 3.2) as usize;
            c.rect((l.w - bw) / 2, by, bw, 3, LINE);
            c.rect((l.w - bw) / 2, by, bw * t as usize / COUNTDOWN_TENTHS as usize, 3, ACCENT);
        }
    }
    (rx, ry, rw, rh)
}

fn paint_all(c: &mut Canvas, f: &Fonts, l: &Layout, items: &[Entry], sprites: &[[Vec<BltPixel>; 2]], selected: usize, offset: usize, left: Option<u32>, message: &str) {
    c.clear();
    let (w, h) = (c.w as f32, c.h as f32);
    c.centered(&f.bold, "Sail", w / 2.0, h * 0.06, h / 13.0, TEXT);
    c.centered(&f.regular, "Choose a system", w / 2.0, h * 0.06 + h / 9.0, h / 34.0, MUTED);
    if items.is_empty() {
        c.centered(&f.medium, "No systems found", w / 2.0, h * 0.45, h / 24.0, TEXT);
        c.centered(&f.regular, "Add a boot loader to the EFI partition", w / 2.0, h * 0.45 + h / 18.0, h / 38.0, MUTED);
        return;
    }
    paint_strip(c, l, sprites, selected, (offset * l.step) as f32);
    paint_arrows_dots(c, l, items.len(), offset, selected);
    paint_footer(c, f, l, items, selected, left, message);
}

fn paint_launching(c: &mut Canvas, f: &Fonts, l: &Layout, items: &[Entry], sprites: &[[Vec<BltPixel>; 2]], selected: usize, offset: usize) -> [(usize, usize, usize, usize); 2] {
    let scroll = (offset * l.step) as f32;
    paint_strip(c, l, sprites, selected, scroll);
    let (sx, sy, sw, sh) = strip_rect(l);
    for i in 0..items.len() {
        if i == selected {
            continue;
        }
        let x = tile_x(l, i, scroll);
        let x0 = x.max(sx as i32) as usize;
        let x1 = (x + l.side as i32).min((sx + sw) as i32);
        if x1 > x0 as i32 {
            c.dim(x0, sy, (x1 - x0 as i32) as usize, sh, 0.7);
        }
    }
    let top = (l.y0 + l.tile) as f32 + l.gap as f32 * 2.6;
    let (rx, ry, rw) = ((l.w as f32 * 0.15) as usize, top as usize, (l.w as f32 * 0.7) as usize);
    let rh = (l.fy + l.fpx * 3.8 - top) as usize;
    clear_rect(c, rx, ry, rw, rh);
    let mut msg = String::from("Starting ");
    msg.push_str(items[selected].name);
    msg.push_str("...");
    c.centered(&f.medium, &msg, l.w as f32 / 2.0, l.fy, l.fpx * 1.25, TEXT);
    let bw = (l.w as f32 * 0.3) as usize;
    c.rect((l.w - bw) / 2, (l.fy + l.fpx * 2.4) as usize, bw, 3, ACCENT);
    [(sx, sy, sw, sh), (rx, ry, rw, rh)]
}

fn start(entry: &Entry) -> Result<(), String> {
    let me = boot::image_handle();
    let dp = boot::open_protocol_exclusive::<DevicePath>(entry.device).map_err(|e| alloc::format!("{:?}", e.status()))?;
    let mut storage = Vec::new();
    let mut b = DevicePathBuilder::with_vec(&mut storage);
    for node in dp.node_iter() {
        b = b.push(&node).map_err(|_| String::from("path"))?;
    }
    let full = b
        .push(&FilePath { path_name: entry.path })
        .and_then(|b| b.finalize())
        .map_err(|_| String::from("path"))?;
    let image = boot::load_image(
        me,
        boot::LoadImageSource::FromDevicePath { device_path: full, boot_policy: uefi::proto::BootPolicy::ExactMatch },
    )
    .map_err(|e| alloc::format!("{:?}", e.status()))?;
    boot::start_image(image).map_err(|e| alloc::format!("{:?}", e.status()))
}

fn tick() {
    boot::stall(Duration::from_millis(10));
}

#[entry]
fn main() -> Status {
    uefi::helpers::init().unwrap();
    let handle = boot::get_handle_for_protocol::<GraphicsOutput>().unwrap();
    let mut gop = boot::open_protocol_exclusive::<GraphicsOutput>(handle).unwrap();
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
    if let Some(m) = best {
        if m.info().resolution() != gop.current_mode_info().resolution() {
            let _ = gop.set_mode(&m);
        }
    }
    let (w, h) = gop.current_mode_info().resolution();
    let mut canvas = Canvas::new(w, h);
    let fonts = load_fonts();
    let items = discover();
    let n = items.len();
    let l = make_layout(w, h, n);
    let sprites = make_sprites(&fonts, &items, &l);

    let mut selected = 0usize;
    let mut offset = 0usize;
    let mut left: Option<u32> = if n == 0 { None } else { Some(COUNTDOWN_TENTHS) };
    let mut message = String::new();
    let mut ticks = 0u32;

    paint_all(&mut canvas, &fonts, &l, &items, &sprites, selected, offset, left, &message);
    blit_rect(&mut gop, &canvas, 0, 0, w, h);

    loop {
        let key = system::with_stdin(|i| i.read_key()).ok().flatten();
        let mut go = false;
        if let Some(k) = key {
            let old = selected;
            let old_offset = offset;
            left = None;
            message.clear();
            match k {
                Key::Special(ScanCode::RIGHT) => selected = (selected + 1) % n.max(1),
                Key::Special(ScanCode::LEFT) => selected = (selected + n.max(1) - 1) % n.max(1),
                Key::Printable(c) => {
                    let ch = char::from(c);
                    if ch == '\r' {
                        go = n > 0;
                    } else if let Some(d) = ch.to_digit(10) {
                        if d >= 1 && (d as usize) <= n { selected = d as usize - 1 }
                    }
                }
                _ => {}
            }
            if n > 0 {
                if selected < offset { offset = selected }
                if selected >= offset + VISIBLE { offset = selected + 1 - VISIBLE }
                let (sx, sy, sw, sh) = strip_rect(&l);
                if offset != old_offset {
                    let (from, to) = ((old_offset * l.step) as f32, (offset * l.step) as f32);
                    const FRAMES: u32 = 9;
                    for fr in 1..=FRAMES {
                        let t = fr as f32 / FRAMES as f32;
                        let e = 1.0 - (1.0 - t) * (1.0 - t) * (1.0 - t);
                        paint_strip(&mut canvas, &l, &sprites, selected, from + (to - from) * e);
                        blit_rect(&mut gop, &canvas, sx, sy, sw, sh);
                        boot::stall(Duration::from_millis(8));
                    }
                } else if selected != old {
                    paint_strip(&mut canvas, &l, &sprites, selected, (offset * l.step) as f32);
                    for i in [old, selected] {
                        let x = tile_x(&l, i, (offset * l.step) as f32).max(sx as i32) as usize;
                        blit_rect(&mut gop, &canvas, x, sy, l.side, sh);
                    }
                }
                for (x, y, rw, rh) in paint_arrows_dots(&mut canvas, &l, n, offset, selected) {
                    blit_rect(&mut gop, &canvas, x, y, rw, rh);
                }
                let (x, y, rw, rh) = paint_footer(&mut canvas, &fonts, &l, &items, selected, left, &message);
                blit_rect(&mut gop, &canvas, x, y, rw, rh);
            }
        } else if let Some(t) = left {
            ticks += 1;
            if ticks >= 10 {
                ticks = 0;
                if t == 0 {
                    go = true;
                } else {
                    left = Some(t - 1);
                    let (x, y, rw, rh) = paint_footer(&mut canvas, &fonts, &l, &items, selected, left, &message);
                    blit_rect(&mut gop, &canvas, x, y, rw, rh);
                }
            }
        }
        if go {
            for (x, y, rw, rh) in paint_launching(&mut canvas, &fonts, &l, &items, &sprites, selected, offset) {
                blit_rect(&mut gop, &canvas, x, y, rw, rh);
            }
            if let Err(e) = start(&items[selected]) {
                message = alloc::format!("Could not start {}: {}", items[selected].name, e);
                left = None;
            }
            paint_all(&mut canvas, &fonts, &l, &items, &sprites, selected, offset, left, &message);
            blit_rect(&mut gop, &canvas, 0, 0, w, h);
        }
        tick();
    }
}
