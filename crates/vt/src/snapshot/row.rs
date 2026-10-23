//! One copied viewport row: values only, so it can be read after the lock is released.

use std::ops::Range;

use rustc_hash::FxHashMap;

use crate::cell::{CellContent, CellWidth, Color, Semantic, Style};
use crate::grid::{RowId, RowRef, SeqNo};
use crate::intern::{ExtrasId, GraphicId, Hyperlink, HyperlinkId, Interner, StyleId};

use super::palette::Palette;

/// A run of columns that share one style.
///
/// Runs follow style **ids**, and the style table deduplicates, so two neighbouring runs
/// never hold equal styles. A uniformly styled row is one run.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct StyleRun {
    /// The columns, half-open.
    pub cols: Range<u16>,
    /// The style, resolved while the lock was held.
    pub style: Style,
}

/// What a copied cell shows.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SnapshotContent {
    /// One scalar; a blank is `' '`.
    Scalar(char),
    /// A grapheme cluster: `len` scalars of [`SnapshotRow::clusters`] from `start`. Read it
    /// with [`SnapshotRow::cluster`].
    Cluster {
        /// The first scalar, as an index into the row's `clusters`.
        start: u32,
        /// How many scalars.
        len: u32,
    },
}

/// One copied cell.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SnapshotCell {
    /// What the cell shows.
    pub content: SnapshotContent,
    /// The cell's role in a double-width pair; skip spacers when drawing.
    pub width: CellWidth,
    /// The shell-integration zone the cell was written in.
    pub semantic: Semantic,
    /// The index of the cell's style in [`SnapshotRow::runs`]; read it with
    /// [`SnapshotRow::style_of`].
    pub run: u16,
    /// The hyperlink covering the cell; resolve it with
    /// [`SnapshotState::hyperlink`](crate::SnapshotState::hyperlink).
    pub hyperlink: Option<HyperlinkId>,
    /// The image covering the cell; resolve it with
    /// [`SnapshotState::placement`](crate::SnapshotState::placement). Always `None` until
    /// images are supported.
    pub graphic: Option<GraphicId>,
}

/// One viewport row, copied out of the grid.
///
/// Everything in it is a value: styles, cluster scalars and hyperlink ids are resolved while
/// the terminal is locked, so the row stays readable after the lock is released. A renderer
/// draws a row like this:
///
/// ```
/// use std::time::Instant;
/// use oneterm_vt::{Config, EventBatch, Size, SnapshotContent, SnapshotState, Terminal};
///
/// let now = Instant::now();
/// let mut term = Terminal::new(Size { rows: 2, cols: 8 }, Config::default());
/// let (mut batch, mut state) = (EventBatch::new(), SnapshotState::new());
/// term.feed("ae\u{301}".as_bytes(), &mut batch, now);
/// term.snapshot_update(&mut state, now);
///
/// let mut drawn = String::new();
/// let row = &state.rows()[0];
/// for cell in &row.cells {
///     if cell.width.is_spacer() {
///         continue; // drawn by the wide cell on its left
///     }
///     let _style = row.style_of(cell); // colours and attributes of this cell
///     match cell.content {
///         SnapshotContent::Scalar(c) => drawn.push(c),
///         SnapshotContent::Cluster { start, len } => drawn.extend(row.cluster(start, len)),
///     }
/// }
/// assert_eq!(drawn.trim_end(), "ae\u{301}");
/// ```
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct SnapshotRow {
    /// The row's id; follows content, so a cached row that still has its id is still valid.
    pub id: RowId,
    /// The grid stamp this copy was taken at; zero for a row that was never written.
    pub seq: SeqNo,
    /// The row continues on the next row (soft wrap).
    pub wrapped: bool,
    /// One cell per column.
    pub cells: Vec<SnapshotCell>,
    /// The style runs, left to right, covering every column.
    pub runs: Vec<StyleRun>,
    /// The scalars of every [`SnapshotContent::Cluster`] in this row.
    pub clusters: Vec<char>,
}

impl SnapshotRow {
    /// The style of `cell`; the default style if its run index is out of range.
    pub fn style_of(&self, cell: &SnapshotCell) -> &Style {
        self.runs
            .get(usize::from(cell.run))
            .map_or(&Style::DEFAULT, |run| &run.style)
    }

    /// The scalars of a cluster; empty when `start` and `len` do not fit this row.
    pub fn cluster(&self, start: u32, len: u32) -> &[char] {
        let Ok(start) = usize::try_from(start) else {
            return &[];
        };
        let Some(end) = usize::try_from(len)
            .ok()
            .and_then(|len| start.checked_add(len))
        else {
            return &[];
        };
        self.clusters.get(start..end).unwrap_or(&[])
    }

    /// Replaces the foreground, background and underline colour of every run with the
    /// `Rgb` that `palette` resolves them to. Attributes are untouched: inverse video,
    /// bold-as-bright and reverse video stay the renderer's. Calling it again changes nothing.
    pub fn map_colors(&mut self, palette: &Palette) {
        for run in &mut self.runs {
            let style = &mut run.style;
            style.fg = Color::Rgb(palette.resolve(style.fg));
            style.bg = Color::Rgb(palette.resolve(style.bg));
            style.underline_color = style
                .underline_color
                .map(|color| Color::Rgb(palette.resolve(color)));
        }
    }

    /// Replaces this row with a copy of `row`, keeping every vector's capacity.
    pub(crate) fn copy_from(
        &mut self,
        row: RowRef<'_>,
        interner: &Interner,
        links: &mut FxHashMap<HyperlinkId, Hyperlink>,
    ) {
        self.id = row.id();
        self.seq = row.seq();
        self.wrapped = row.wrapped();
        self.cells.clear();
        self.runs.clear();
        self.clusters.clear();
        let mut open: Option<StyleId> = None;
        for cell in row.cells() {
            // A row has at most `MAX_COLS` cells, so every column and run index fits `u16`.
            let col = u16::try_from(self.cells.len()).unwrap_or(u16::MAX);
            if open != Some(cell.style_id()) {
                open = Some(cell.style_id());
                self.runs.push(StyleRun {
                    cols: col..col,
                    style: *interner.resolve_style(cell.style_id()),
                });
            }
            let content = match cell.content() {
                CellContent::Scalar(c) => SnapshotContent::Scalar(c),
                CellContent::Grapheme(id) => {
                    let start = self.clusters.len();
                    self.clusters.extend(interner.resolve_grapheme(id).chars());
                    SnapshotContent::Cluster {
                        start: u32::try_from(start).unwrap_or(u32::MAX),
                        len: u32::try_from(self.clusters.len().saturating_sub(start))
                            .unwrap_or(u32::MAX),
                    }
                }
            };
            let extras = (cell.extras_id() != ExtrasId::NONE)
                .then(|| *interner.resolve_extras(cell.extras_id()));
            let hyperlink = extras.and_then(|extras| extras.hyperlink);
            if let Some(id) = hyperlink
                && !links.contains_key(&id)
                && let Some(link) = interner.hyperlinks.resolve(id)
            {
                links.insert(id, link.clone());
            }
            self.cells.push(SnapshotCell {
                content,
                width: cell.width(),
                semantic: cell.semantic(),
                run: u16::try_from(self.runs.len().saturating_sub(1)).unwrap_or(u16::MAX),
                hyperlink,
                graphic: extras.and_then(|extras| extras.graphic),
            });
            if let Some(run) = self.runs.last_mut() {
                run.cols.end = col.saturating_add(1);
            }
        }
    }
}
