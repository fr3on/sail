//! Software canvas: anti-aliased shapes, cached glyphs, integer blending.

use alloc::collections::BTreeMap;
use alloc::rc::Rc;
use alloc::vec;
use alloc::vec::Vec;
use core::cell::RefCell;
use fontdue::{Font, FontSettings, Metrics};
use uefi::proto::console::gop::BltPixel;

pub const BG: u32 = 0x000000;
pub const PANEL: u32 = 0x11151c;
pub const LINE: u32 = 0x2a313b;
pub const TEXT: u32 = 0xf0f3f6;
pub const MUTED: u32 = 0x8b949e;
pub const ACCENT: u32 = 0x3b82f6;
pub const ERROR: u32 = 0xf85149;
pub const OK: u32 = 0x3fb950;

struct Glyph {
    m: Metrics,
    bmp: Vec<u8>,
}

/// A font plus a cache of rasterized glyphs, so a repaint never rasterizes twice.
pub struct Typeface {
    font: Font,
    cache: RefCell<BTreeMap<(char, u32), Rc<Glyph>>>,
}

impl Typeface {
    fn new(bytes: &[u8]) -> Option<Self> {
        let font = Font::from_bytes(bytes, FontSettings::default()).ok()?;
        Some(Self {
            font,
            cache: RefCell::new(BTreeMap::new()),
        })
    }

    fn glyph(&self, ch: char, px: f32) -> Rc<Glyph> {
        let key = (ch, px.to_bits());
        if let Some(g) = self.cache.borrow().get(&key) {
            return g.clone();
        }
        let (m, bmp) = self.font.rasterize(ch, px);
        let g = Rc::new(Glyph { m, bmp });
        self.cache.borrow_mut().insert(key, g.clone());
        g
    }

    pub fn measure(&self, text: &str, px: f32) -> f32 {
        text.chars().map(|c| self.glyph(c, px).m.advance_width).sum()
    }

    fn ascent(&self, px: f32) -> f32 {
        self.font
            .horizontal_line_metrics(px)
            .map(|m| m.ascent)
            .unwrap_or(px * 0.8)
    }
}

pub struct Fonts {
    pub regular: Typeface,
    pub medium: Typeface,
    pub bold: Typeface,
}

impl Fonts {
    /// `None` if an embedded font fails to parse; the caller falls back to text mode.
    pub fn load() -> Option<Self> {
        Some(Self {
            regular: Typeface::new(include_bytes!("../assets/DMSans-Regular.ttf"))?,
            medium: Typeface::new(include_bytes!("../assets/DMSans-Medium.ttf"))?,
            bold: Typeface::new(include_bytes!("../assets/DMSans-Bold.ttf"))?,
        })
    }
}

#[inline]
fn rgb(c: u32) -> (u32, u32, u32) {
    ((c >> 16) & 255, (c >> 8) & 255, c & 255)
}

/// Blend `color` over `under` with `a` in 0..=256.
#[inline]
fn blend(under: BltPixel, color: (u32, u32, u32), a: u32) -> BltPixel {
    let ia = 256 - a;
    let mix = |u: u8, c: u32| ((u as u32 * ia + c * a) >> 8) as u8;
    BltPixel::new(
        mix(under.red, color.0),
        mix(under.green, color.1),
        mix(under.blue, color.2),
    )
}

pub struct Canvas {
    pub w: usize,
    pub h: usize,
    pub buf: Vec<BltPixel>,
}

impl Canvas {
    pub fn new(w: usize, h: usize) -> Self {
        Self {
            w,
            h,
            buf: vec![BltPixel::from(BG); w * h],
        }
    }

    pub fn clear(&mut self) {
        self.buf.fill(BltPixel::from(BG));
    }

    #[inline]
    pub fn put(&mut self, x: i32, y: i32, color: u32, alpha: f32) {
        if x < 0 || y < 0 || x as usize >= self.w || y as usize >= self.h || alpha <= 0.0 {
            return;
        }
        let i = y as usize * self.w + x as usize;
        if alpha >= 1.0 {
            self.buf[i] = BltPixel::from(color);
        } else {
            self.buf[i] = blend(self.buf[i], rgb(color), (alpha * 256.0) as u32);
        }
    }

    pub fn dim(&mut self, x: usize, y: usize, w: usize, h: usize, t: f32) {
        let a = (t * 256.0) as u32;
        for row in y..(y + h).min(self.h) {
            for col in x..(x + w).min(self.w) {
                let i = row * self.w + col;
                self.buf[i] = blend(self.buf[i], (0, 0, 0), a);
            }
        }
    }

    pub fn rect(&mut self, x: usize, y: usize, w: usize, h: usize, color: u32) {
        let c = BltPixel::from(color);
        for row in y..(y + h).min(self.h) {
            let start = row * self.w + x.min(self.w);
            let end = row * self.w + (x + w).min(self.w);
            self.buf[start..end].fill(c);
        }
    }

    /// Rounded rectangle. The square root is only needed in the corners.
    pub fn rounded(&mut self, x: f32, y: f32, w: f32, h: f32, r: f32, color: u32) {
        let (cx, cy) = (x + w / 2.0, y + h / 2.0);
        let (hx, hy) = (w / 2.0 - r, h / 2.0 - r);
        for py in (y as i32 - 1)..(y + h) as i32 + 2 {
            for px in (x as i32 - 1)..(x + w) as i32 + 2 {
                let dx = (libm::fabsf(px as f32 + 0.5 - cx) - hx).max(0.0);
                let dy = (libm::fabsf(py as f32 + 0.5 - cy) - hy).max(0.0);
                let d = if dx > 0.0 && dy > 0.0 {
                    libm::sqrtf(dx * dx + dy * dy)
                } else {
                    dx.max(dy)
                } - r;
                self.put(px, py, color, 0.5 - d);
            }
        }
    }

    pub fn circle(&mut self, cx: f32, cy: f32, r: f32, color: u32) {
        self.rounded(cx - r, cy - r, r * 2.0, r * 2.0, r, color);
    }

    pub fn ring(&mut self, cx: f32, cy: f32, r: f32, thick: f32, color: u32) {
        for py in (cy - r) as i32 - 2..(cy + r) as i32 + 3 {
            for px in (cx - r) as i32 - 2..(cx + r) as i32 + 3 {
                let (dx, dy) = (px as f32 + 0.5 - cx, py as f32 + 0.5 - cy);
                let d = libm::fabsf(libm::sqrtf(dx * dx + dy * dy) - r) - thick / 2.0;
                self.put(px, py, color, 0.5 - d);
            }
        }
    }

    pub fn line(&mut self, x0: f32, y0: f32, x1: f32, y1: f32, thick: f32, color: u32) {
        let (minx, maxx) = (x0.min(x1) - thick - 2.0, x0.max(x1) + thick + 2.0);
        let (miny, maxy) = (y0.min(y1) - thick - 2.0, y0.max(y1) + thick + 2.0);
        let (dx, dy) = (x1 - x0, y1 - y0);
        let len2 = (dx * dx + dy * dy).max(1e-6);
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

    #[allow(clippy::too_many_arguments)]
    pub fn arc(&mut self, cx: f32, cy: f32, r: f32, a0: f32, a1: f32, thick: f32, color: u32) {
        let steps = 28;
        let at = |a: f32| (cx + r * libm::cosf(a), cy + r * libm::sinf(a));
        let (mut px, mut py) = at(a0);
        for i in 1..=steps {
            let (x, y) = at(a0 + (a1 - a0) * i as f32 / steps as f32);
            self.line(px, py, x, y, thick, color);
            (px, py) = (x, y);
        }
    }

    pub fn text(&mut self, face: &Typeface, text: &str, x: f32, top: f32, px: f32, color: u32) {
        let baseline = top + face.ascent(px);
        let mut pen = x;
        for ch in text.chars() {
            let g = face.glyph(ch, px);
            let gx = pen as i32 + g.m.xmin;
            let gy = (baseline - g.m.height as f32 - g.m.ymin as f32) as i32;
            for row in 0..g.m.height {
                for col in 0..g.m.width {
                    let a = g.bmp[row * g.m.width + col];
                    if a != 0 {
                        self.put(gx + col as i32, gy + row as i32, color, a as f32 / 255.0);
                    }
                }
            }
            pen += g.m.advance_width;
        }
    }

    pub fn centered(&mut self, face: &Typeface, text: &str, cx: f32, top: f32, px: f32, color: u32) {
        let w = face.measure(text, px);
        self.text(face, text, cx - w / 2.0, top, px, color);
    }
}
