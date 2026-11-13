//! Screen geometry. All sizes derive from the screen height and width, and every
//! subtraction saturates so tiny or odd modes cannot underflow.

#[derive(Clone, Debug)]
pub struct Layout {
    pub w: usize,
    pub h: usize,
    pub tile: usize,
    pub gap: usize,
    pub shown: usize,
    pub x0: usize,
    pub y0: usize,
    pub row_w: usize,
    pub side: usize,
    pub pad: usize,
    pub step: usize,
    pub fpx: f32,
    pub fy: f32,
}

pub const VISIBLE: usize = 4;

impl Layout {
    pub fn new(w: usize, h: usize, n: usize) -> Self {
        let shown = n.clamp(1, VISIBLE);
        let gap = h / 28;
        let avail_w = (w as f32 * 0.80) as usize;
        let tile = (avail_w.saturating_sub(gap * (shown - 1)) / shown)
            .min(h * 25 / 100)
            .max(64);
        let row_w = tile * shown + gap * (shown - 1);
        Layout {
            w,
            h,
            tile,
            gap,
            shown,
            x0: w.saturating_sub(row_w) / 2,
            y0: h * 30 / 100 + (h * 40 / 100).saturating_sub(tile) / 2,
            row_w,
            side: tile + 8,
            pad: (tile as f32 / 13.0).max(11.0) as usize + 4,
            step: tile + gap,
            fpx: h as f32 / 38.0,
            fy: h as f32 * 0.90,
        }
    }

    /// The rectangle the tile strip occupies: x, y, width, height.
    pub fn strip_rect(&self) -> (usize, usize, usize, usize) {
        let x = self.x0.saturating_sub(4);
        let y = self.y0.saturating_sub(4 + self.pad);
        (x, y, self.row_w + 8, self.side + self.pad)
    }

    pub fn tile_x(&self, i: usize, scroll_px: f32) -> i32 {
        (self.x0 as f32 + i as f32 * self.step as f32 - scroll_px) as i32 - 4
    }

    /// Top and height of the tool row.
    pub fn dock_geometry(&self) -> (f32, f32) {
        (
            (self.y0 + self.tile) as f32 + self.gap as f32 * 3.6,
            self.h as f32 / 24.0,
        )
    }

    pub fn footer_top(&self) -> f32 {
        let (y, h) = self.dock_geometry();
        y + h + self.gap as f32 * 0.8
    }

    /// The footer rectangle: x, y, width, height.
    pub fn footer_rect(&self) -> (usize, usize, usize, usize) {
        let top = self.footer_top();
        let rh = (self.fy + self.fpx * 3.8 - top).max(0.0) as usize;
        (
            (self.w as f32 * 0.15) as usize,
            top as usize,
            (self.w as f32 * 0.7) as usize,
            rh,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn common_modes_fit_on_screen() {
        for (w, h) in [(640, 480), (1024, 768), (1920, 1080), (1920, 1200)] {
            for n in [0, 1, 2, 4, 7, 32] {
                let l = Layout::new(w, h, n);
                assert!(l.x0 + l.row_w <= w, "{w}x{h} n={n}");
                let (_, y, _, sh) = l.strip_rect();
                assert!(y + sh <= h, "{w}x{h} n={n}");
                let (_, fy, _, fh) = l.footer_rect();
                assert!(fy + fh <= h, "{w}x{h} n={n}");
            }
        }
    }

    #[test]
    fn tiny_modes_do_not_underflow() {
        for (w, h) in [(0, 0), (1, 1), (80, 25), (100, 60)] {
            let l = Layout::new(w, h, 7);
            let _ = (l.strip_rect(), l.footer_rect(), l.dock_geometry());
        }
    }

    #[test]
    fn tiles_never_overlap() {
        let l = Layout::new(1920, 1200, 4);
        assert!(l.tile_x(1, 0.0) - l.tile_x(0, 0.0) >= l.side as i32);
    }
}
