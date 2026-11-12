//! Which rows of a long library are mounted, and which pages of games are
//! loaded, from the scroll position: the arithmetic behind
//! [`VirtualLibrary`](super::VirtualLibrary), kept apart so it can be tested
//! without a window. Rows have fixed heights, as the webview's do
//! (`gameCardLayout.ts`, `LibraryRow.tsx`).

use std::ops::RangeInclusive;

/// The grid's gap between cards and rows.
pub const GRID_GAP: f64 = 16.0;
/// A grid row: a card and the gap below it.
pub const GRID_ROW: f64 = 312.0;
/// The narrowest a card gets before a column goes.
pub const MIN_CARD: f64 = 176.0;
/// A list row.
pub const LIST_ROW: f64 = 44.0;
/// The list's sticky header, above its rows.
pub const LIST_HEADER: f64 = 32.0;
/// Games per page the data layer loads (`caskade_ui_data::PAGE_LIMIT`).
pub const PAGE: usize = caskade_ui_data::PAGE_LIMIT as usize;
/// Pages kept beyond the ones on screen, each way: scrolling back a little
/// shows games without loading them again, and memory stays bounded.
pub const KEEP_PAGES: usize = 2;

/// Cards or a compact list.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Layout {
    #[default]
    Grid,
    List,
}

impl Layout {
    pub fn row_height(self) -> f64 {
        match self {
            Self::Grid => GRID_ROW,
            Self::List => LIST_ROW,
        }
    }

    /// Where the rows start: under the list's header.
    pub fn rows_top(self) -> f64 {
        match self {
            Self::Grid => 0.0,
            Self::List => LIST_HEADER,
        }
    }

    /// Rows mounted beyond the viewport, each way: one keeps keyboard and
    /// pad moves on mounted games. The list keeps two: every scroll step
    /// mounts new rows, so each one more costs the frame (the webview's
    /// six took the list's p95 frame from 46 to 49 ms on the CPU; #111).
    pub fn overscan(self) -> usize {
        match self {
            Self::Grid => 1,
            Self::List => 2,
        }
    }

    /// Games per row in a viewport `width` wide.
    pub fn columns(self, width: f64) -> usize {
        match self {
            Self::Grid => (((width + GRID_GAP) / (MIN_CARD + GRID_GAP)).floor() as usize).max(1),
            Self::List => 1,
        }
    }
}

/// The library's shape on screen now.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Frame {
    pub layout: Layout,
    pub total: usize,
    /// The scroller's size.
    pub width: f64,
    pub height: f64,
    /// How far it's scrolled.
    pub scroll_top: f64,
}

impl Frame {
    pub fn columns(&self) -> usize {
        self.layout.columns(self.width)
    }

    pub fn rows(&self) -> usize {
        self.total.div_ceil(self.columns())
    }

    /// The scroll content's height: every row, and the header.
    pub fn content_height(&self) -> f64 {
        self.layout.rows_top() + self.rows() as f64 * self.layout.row_height()
    }

    /// The rows on screen, with the overscan each side; empty with no
    /// games.
    pub fn window(&self) -> Option<RangeInclusive<usize>> {
        let rows = self.rows();
        if rows == 0 {
            return None;
        }
        let row = self.layout.row_height();
        let top = (self.scroll_top - self.layout.rows_top()).max(0.0);
        let bottom = (self.scroll_top + self.height - self.layout.rows_top()).max(0.0);
        let first = (top / row).floor() as usize;
        // A row that only touches the bottom edge isn't on screen.
        let last = ((bottom / row).ceil() as usize)
            .saturating_sub(1)
            .max(first);
        let overscan = self.layout.overscan();
        Some(first.saturating_sub(overscan)..=(last + overscan).min(rows - 1))
    }

    /// The rows to mount: the window, and the focused game's row wherever
    /// it is, so focus isn't lost when it scrolls out of view.
    pub fn mounted(&self, focused: Option<usize>) -> Vec<usize> {
        let Some(window) = self.window() else {
            return Vec::new();
        };
        let mut rows: Vec<usize> = window.collect();
        if let Some(index) = focused.filter(|&index| index < self.total) {
            let row = index / self.columns();
            if !rows.contains(&row) {
                rows.push(row);
                rows.sort_unstable();
            }
        }
        rows
    }

    /// The games of row `row`.
    pub fn games_in(&self, row: usize) -> std::ops::Range<usize> {
        let columns = self.columns();
        let start = row * columns;
        start..(start + columns).min(self.total)
    }

    /// The pages the mounted rows need.
    pub fn needed_pages(&self, focused: Option<usize>) -> Vec<usize> {
        let mut pages: Vec<usize> = self
            .mounted(focused)
            .into_iter()
            .flat_map(|row| {
                let games = self.games_in(row);
                (games.start / PAGE)..=((games.end.max(1) - 1) / PAGE)
            })
            .collect();
        pages.dedup();
        pages
    }

    /// Whether page `page` is worth keeping: a needed one, or within
    /// [`KEEP_PAGES`] of one. However far focus and the viewport are apart,
    /// that's a few pages each.
    pub fn keeps(&self, page: usize, focused: Option<usize>) -> bool {
        self.needed_pages(focused)
            .iter()
            .any(|&needed| page.abs_diff(needed) <= KEEP_PAGES)
    }

    /// The scroll position that shows game `index`'s row at the top.
    pub fn top_of(&self, index: usize) -> f64 {
        let row = index / self.columns();
        let top = row as f64 * self.layout.row_height();
        top.min((self.content_height() - self.height).max(0.0))
    }

    /// The first game whose row is on screen (below the list's header).
    pub fn first_shown(&self) -> usize {
        let row = ((self.scroll_top / self.layout.row_height()).ceil() as usize)
            .min(self.rows().saturating_sub(1));
        row * self.columns()
    }

    /// Games per screen: whole rows that fit, at least one.
    pub fn per_page(&self) -> usize {
        let rows = ((self.height - self.layout.rows_top()) / self.layout.row_height()).floor();
        (rows as usize).max(1) * self.columns()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grid(total: usize, scroll_top: f64) -> Frame {
        Frame {
            layout: Layout::Grid,
            total,
            width: 1000.0,
            height: 800.0,
            scroll_top,
        }
    }

    #[test]
    fn columns_follow_the_webviews_card_layout() {
        // floor((w + 16) / 192), at least one.
        assert_eq!(Layout::Grid.columns(1000.0), 5);
        assert_eq!(Layout::Grid.columns(176.0), 1);
        assert_eq!(Layout::Grid.columns(100.0), 1);
        assert_eq!(Layout::Grid.columns(1904.0), 10);
        assert_eq!(Layout::List.columns(1904.0), 1);
    }

    #[test]
    fn the_window_is_the_rows_on_screen_and_one_more_each_way() {
        let frame = grid(10_000, 0.0);
        assert_eq!(frame.rows(), 2000);
        // 800 px shows rows 0, 1 and part of 2; one more below.
        assert_eq!(frame.window(), Some(0..=3));
        let frame = grid(10_000, 312.0 * 1000.0 + 10.0);
        // Rows 1000 to 1002 on screen, one more each side.
        assert_eq!(frame.window(), Some(999..=1003));
        // At the end, no rows past the last.
        let end = grid(10_000, frame.content_height() - 800.0);
        assert_eq!(*end.window().unwrap().end(), 1999);
        assert_eq!(grid(0, 0.0).window(), None);
    }

    #[test]
    fn mounted_games_stay_bounded_however_far_it_scrolls() {
        let mut most = 0;
        let mut top = 0.0;
        let content = grid(10_000, 0.0).content_height();
        while top < content {
            let frame = grid(10_000, top);
            let games: usize = frame
                .mounted(Some(0))
                .iter()
                .map(|&row| frame.games_in(row).len())
                .sum();
            most = most.max(games);
            top += 97.0;
        }
        // Six rows of five (four on screen, one each side), and the focused
        // card's row pinned far above.
        assert!(most <= 7 * 5, "{most}");
    }

    #[test]
    fn the_focused_row_stays_mounted_and_its_page_loaded() {
        let frame = grid(10_000, 312.0 * 500.0);
        let rows = frame.mounted(Some(7));
        assert_eq!(rows[0], 1, "the focused card's row");
        assert!(rows.contains(&500));
        assert_eq!(frame.needed_pages(Some(7)), [0, 24, 25]);
        let kept: Vec<usize> = (0..100)
            .filter(|&page| frame.keeps(page, Some(7)))
            .collect();
        assert_eq!(kept, [0, 1, 2, 22, 23, 24, 25, 26, 27]);
    }

    #[test]
    fn list_rows_start_below_the_header() {
        let frame = Frame {
            layout: Layout::List,
            total: 300,
            width: 1000.0,
            height: 440.0,
            scroll_top: 0.0,
        };
        assert_eq!(frame.content_height(), 32.0 + 300.0 * 44.0);
        // 408 px of rows: 0..=9, and two more.
        assert_eq!(frame.window(), Some(0..=11));
        assert_eq!(frame.per_page(), 9);
    }

    #[test]
    fn jumps_land_a_games_row_at_the_top() {
        let frame = grid(10_000, 0.0);
        assert_eq!(frame.top_of(12), 312.0 * 2.0);
        // Not past the end.
        assert_eq!(frame.top_of(9_999), frame.content_height() - 800.0);
        assert_eq!(grid(10_000, 312.0 * 3.0).first_shown(), 15);
        assert_eq!(frame.per_page(), 10);
    }
}
