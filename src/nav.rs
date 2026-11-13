//! Selection, scrolling and countdown state, kept free of drawing.

use crate::layout::VISIBLE;

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Dir {
    Left,
    Right,
    Up,
    Down,
}

#[derive(Clone, Debug)]
pub struct Nav {
    pub n: usize,
    pub tools: usize,
    pub selected: usize,
    pub offset: usize,
    /// `Some(i)` while the tool row has focus.
    pub dock: Option<usize>,
}

pub fn offset_for(selected: usize) -> usize {
    (selected + 1).saturating_sub(VISIBLE)
}

impl Nav {
    pub fn new(n: usize, tools: usize, selected: usize) -> Self {
        let selected = if n == 0 { 0 } else { selected.min(n - 1) };
        Nav {
            n,
            tools,
            selected,
            offset: offset_for(selected),
            dock: None,
        }
    }

    pub fn press(&mut self, d: Dir) {
        match (d, self.dock) {
            (Dir::Down, None) if self.tools > 0 => self.dock = Some(0),
            (Dir::Up, _) => self.dock = None,
            (Dir::Right, Some(i)) => self.dock = Some((i + 1) % self.tools),
            (Dir::Left, Some(i)) => self.dock = Some((i + self.tools - 1) % self.tools),
            (Dir::Right, None) if self.n > 0 => self.selected = (self.selected + 1) % self.n,
            (Dir::Left, None) if self.n > 0 => self.selected = (self.selected + self.n - 1) % self.n,
            _ => {}
        }
        self.fix_offset();
    }

    /// Number keys 1..=9 select an entry directly (only outside the tool row).
    pub fn digit(&mut self, d: u32) {
        if self.dock.is_none() && d >= 1 && d as usize <= self.n {
            self.selected = d as usize - 1;
            self.fix_offset();
        }
    }

    fn fix_offset(&mut self) {
        if self.selected < self.offset {
            self.offset = self.selected;
        }
        if self.selected >= self.offset + VISIBLE {
            self.offset = offset_for(self.selected);
        }
    }
}

/// One countdown tick (100 ms). `None` means the countdown has run out.
pub fn countdown_step(left: u32) -> Option<u32> {
    left.checked_sub(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn right_wraps_and_scrolls() {
        let mut nav = Nav::new(7, 3, 0);
        for _ in 0..4 {
            nav.press(Dir::Right);
        }
        assert_eq!((nav.selected, nav.offset), (4, 1));
        for _ in 0..3 {
            nav.press(Dir::Right);
        }
        assert_eq!((nav.selected, nav.offset), (0, 0));
    }

    #[test]
    fn left_from_first_goes_to_last() {
        let mut nav = Nav::new(7, 3, 0);
        nav.press(Dir::Left);
        assert_eq!((nav.selected, nav.offset), (6, 3));
    }

    #[test]
    fn dock_focus_and_wrap() {
        let mut nav = Nav::new(3, 3, 1);
        nav.press(Dir::Down);
        assert_eq!(nav.dock, Some(0));
        nav.press(Dir::Left);
        assert_eq!(nav.dock, Some(2));
        nav.press(Dir::Right);
        assert_eq!(nav.dock, Some(0));
        nav.press(Dir::Up);
        assert_eq!(nav.dock, None);
        assert_eq!(nav.selected, 1);
    }

    #[test]
    fn digits_are_bounded_and_ignored_in_dock() {
        let mut nav = Nav::new(3, 2, 0);
        nav.digit(0);
        nav.digit(9);
        assert_eq!(nav.selected, 0);
        nav.digit(3);
        assert_eq!(nav.selected, 2);
        nav.press(Dir::Down);
        nav.digit(1);
        assert_eq!(nav.selected, 2);
    }

    #[test]
    fn empty_list_never_panics() {
        let mut nav = Nav::new(0, 2, 5);
        for d in [Dir::Left, Dir::Right, Dir::Down, Dir::Up] {
            nav.press(d);
        }
        nav.digit(1);
        assert_eq!(nav.selected, 0);
    }

    #[test]
    fn last_used_beyond_view_starts_scrolled() {
        let nav = Nav::new(7, 3, 6);
        assert_eq!((nav.selected, nav.offset), (6, 3));
    }

    #[test]
    fn countdown_fires_after_exactly_n_ticks() {
        let mut left = 3;
        let mut ticks = 0;
        while let Some(l) = countdown_step(left) {
            left = l;
            ticks += 1;
        }
        assert_eq!(ticks, 3);
    }
}
