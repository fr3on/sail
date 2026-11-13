//! Logos (bilinear-scaled alpha masks), the Windows mark and the tool-row icons.

use crate::entries::Kind;
use crate::gfx::{Canvas, Fonts};

const MASK: usize = 192;
static FEDORA: &[u8] = include_bytes!("../assets/icons/fedora.a");
static UBUNTU: &[u8] = include_bytes!("../assets/icons/ubuntu.a");
static DEBIAN: &[u8] = include_bytes!("../assets/icons/debian.a");
static ARCH: &[u8] = include_bytes!("../assets/icons/archlinux.a");
static SUSE: &[u8] = include_bytes!("../assets/icons/opensuse.a");
static MINT: &[u8] = include_bytes!("../assets/icons/linuxmint.a");

fn letter(c: &mut Canvas, f: &Fonts, ch: char, cx: f32, cy: f32, size: f32) {
    let mut buf = [0u8; 4];
    let s: &str = ch.encode_utf8(&mut buf);
    let px = size * 0.62;
    let w = f.bold.measure(s, px);
    c.text(&f.bold, s, cx - w / 2.0, cy - px * 0.52, px, 0xffffff);
}

fn mask(c: &mut Canvas, data: &[u8], cx: f32, cy: f32, size: f32, color: u32) {
    // Guard against a truncated asset instead of indexing out of bounds.
    if data.len() < MASK * MASK {
        return;
    }
    let (x0, y0) = (cx - size / 2.0, cy - size / 2.0);
    let at = |x: i32, y: i32| {
        data[y.clamp(0, MASK as i32 - 1) as usize * MASK + x.clamp(0, MASK as i32 - 1) as usize] as f32 / 255.0
    };
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

pub fn icon(c: &mut Canvas, f: &Fonts, kind: Kind, cx: f32, cy: f32, s: f32) {
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
        Kind::Generic(ch) => {
            c.circle(cx, cy, s / 2.0, 0x374151);
            letter(c, f, ch, cx, cy, s);
        }
        Kind::Other => {
            c.circle(cx, cy, s / 2.0, 0x4b5563);
            letter(c, f, '?', cx, cy, s);
        }
    }
}

pub fn tool_icon(c: &mut Canvas, i: usize, cx: f32, cy: f32, r: f32, color: u32) {
    let t = (r * 0.2).max(1.6);
    let pi = core::f32::consts::PI;
    match i {
        0 => {
            c.ring(cx, cy, r * 0.5, t, color);
            for k in 0..8 {
                let a = k as f32 * pi / 4.0;
                let (cs, sn) = (libm::cosf(a), libm::sinf(a));
                c.line(
                    cx + cs * r * 0.72,
                    cy + sn * r * 0.72,
                    cx + cs * r,
                    cy + sn * r,
                    t * 1.5,
                    color,
                );
            }
            c.ring(cx, cy, r * 0.74, t, color);
        }
        1 => {
            let end = pi * 1.28;
            c.arc(cx, cy, r * 0.82, -pi * 0.35, end, t, color);
            let (cs, sn) = (libm::cosf(end), libm::sinf(end));
            let (px, py) = (cx + cs * r * 0.82, cy + sn * r * 0.82);
            let (tx, ty) = (-sn, cs);
            let tip = (px + tx * r * 0.45, py + ty * r * 0.45);
            let (b1, b2) = (
                (px + cs * r * 0.42, py + sn * r * 0.42),
                (px - cs * r * 0.42, py - sn * r * 0.42),
            );
            c.line(b1.0, b1.1, tip.0, tip.1, t, color);
            c.line(b2.0, b2.1, tip.0, tip.1, t, color);
            c.line(b1.0, b1.1, b2.0, b2.1, t, color);
        }
        _ => {
            c.arc(
                cx,
                cy + r * 0.08,
                r * 0.82,
                -pi * 0.5 + 0.7,
                -pi * 0.5 + 2.0 * pi - 0.7,
                t,
                color,
            );
            c.line(cx, cy - r * 0.95, cx, cy - r * 0.1, t * 1.2, color);
        }
    }
}
