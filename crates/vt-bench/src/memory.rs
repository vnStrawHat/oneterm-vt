//! Tier 5: live heap after filling the scrollback, measured by a counting allocator.
//!
//! This is the only module with `unsafe` in the workspace until the pseudo-console lands.

#![allow(
    unsafe_code,
    reason = "GlobalAlloc is an unsafe trait; this module only forwards to System"
)]

use std::alloc::{GlobalAlloc, Layout, System};
use std::fmt::Write as _;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

use oneterm_vt::{Config, EventBatch, Size, Terminal};

use crate::fixtures::{COLUMNS, LINES};
use crate::perf_history::MemoryCost;
use crate::throughput::CHUNK;

/// Rows of scrollback each kind fills.
pub(crate) const MEMORY_ROWS: usize = 10_000;

/// Bytes currently allocated through [`CountingAllocator`].
static LIVE_BYTES: AtomicUsize = AtomicUsize::new(0);

/// The system allocator plus a count of the bytes it holds. No allocation, no panic inside.
pub(crate) struct CountingAllocator;

// SAFETY: every method forwards to `System` with the arguments it received and returns
// `System`'s pointer unchanged, so `System`'s guarantees carry over. The counter updates are
// plain atomic arithmetic on `layout.size()` and never touch the memory.
unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        // SAFETY: the caller upholds `GlobalAlloc::alloc`'s contract for `layout`.
        let ptr = unsafe { System.alloc(layout) };
        if !ptr.is_null() {
            LIVE_BYTES.fetch_add(layout.size(), Ordering::Relaxed);
        }
        ptr
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: the caller passes a pointer this allocator returned for the same `layout`,
        // and every pointer it returned came from `System`.
        unsafe { System.dealloc(ptr, layout) };
        LIVE_BYTES.fetch_sub(layout.size(), Ordering::Relaxed);
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        // SAFETY: the caller upholds `GlobalAlloc::realloc`'s contract: `ptr` came from this
        // allocator with `layout`, and `new_size` is valid for `layout.align()`.
        let moved = unsafe { System.realloc(ptr, layout, new_size) };
        if !moved.is_null() {
            LIVE_BYTES.fetch_add(new_size, Ordering::Relaxed);
            LIVE_BYTES.fetch_sub(layout.size(), Ordering::Relaxed);
        }
        moved
    }
}

/// Bytes allocated and not yet freed.
pub(crate) fn live_heap_bytes() -> usize {
    LIVE_BYTES.load(Ordering::Relaxed)
}

/// A kind of row and its generator.
struct RowKind {
    name: &'static str,
    make: fn() -> Vec<u8>,
}

/// One full 160-column row of each kind, ending in `CR LF`.
const KINDS: [RowKind; 4] = [
    RowKind {
        name: "plain",
        make: plain_row,
    },
    RowKind {
        name: "unicode",
        make: unicode_row,
    },
    RowKind {
        name: "styled",
        make: styled_row,
    },
    RowKind {
        name: "mixed",
        make: mixed_row,
    },
];

fn letter(index: usize) -> char {
    char::from(b'a' + (index % 26) as u8)
}

fn plain_row() -> Vec<u8> {
    let mut row = vec![b'.'; COLUMNS];
    row.extend_from_slice(b"\r\n");
    row
}

fn unicode_row() -> Vec<u8> {
    let mut row = "\u{56FD}".repeat(COLUMNS / 2).into_bytes();
    row.extend_from_slice(b"\r\n");
    row
}

fn styled_row() -> Vec<u8> {
    let mut row = String::new();
    for k in 0..COLUMNS {
        // Writing to a `String` cannot fail.
        let _ = write!(
            row,
            "\x1b[38;2;{};{};{}m{}",
            k % 256,
            3 * k % 256,
            7 * k % 256,
            letter(k)
        );
    }
    row.push_str("\x1b[0m\r\n");
    row.into_bytes()
}

fn mixed_row() -> Vec<u8> {
    let mut row = String::new();
    for k in 0..COLUMNS {
        if k % 2 == 0 {
            let _ = write!(row, "\x1b[48;5;{}m", k % 256);
        }
        row.push(if k % 8 == 0 { ' ' } else { letter(k) });
    }
    row.push_str("\x1b[0m\r\n");
    row.into_bytes()
}

/// Fills a fresh terminal with `rows` rows from `make_row` and returns the heap it holds.
fn heap_after_filling(make_row: fn() -> Vec<u8>, rows: usize) -> u64 {
    let stream = make_row().repeat(rows);
    let before = live_heap_bytes();
    let size = Size {
        rows: LINES as u16,
        cols: COLUMNS as u16,
    };
    let mut term = Terminal::new(size, Config::default());
    let mut batch = EventBatch::new();
    let now = Instant::now();
    for chunk in stream.chunks(CHUNK) {
        term.feed(chunk, &mut batch, now);
    }
    let after = live_heap_bytes();
    std::hint::black_box((&term, &batch));
    after.saturating_sub(before) as u64
}

/// Tier 5: the live heap of a terminal with `MEMORY_ROWS` rows of scrollback, per kind of row.
pub(crate) fn measure_memory() -> Vec<MemoryCost> {
    measure_rows(MEMORY_ROWS)
}

fn measure_rows(rows: usize) -> Vec<MemoryCost> {
    KINDS
        .iter()
        .map(|kind| {
            let heap_bytes = heap_after_filling(kind.make, rows);
            MemoryCost {
                content: kind.name.to_owned(),
                heap_bytes,
                heap_bytes_per_row: heap_bytes as f64 / rows as f64,
            }
        })
        .collect()
}

pub(crate) fn render_table(rows: &[MemoryCost]) -> String {
    let mut table = String::from("| Content | heap bytes | bytes/row |\n|---|---:|---:|\n");
    for row in rows {
        // Writing to a String cannot fail.
        let _ = writeln!(
            table,
            "| `{}` | {} | {:.1} |",
            row.content, row.heap_bytes, row.heap_bytes_per_row
        );
    }
    table
}

#[cfg(test)]
mod tests {
    use std::sync::{Mutex, PoisonError};

    use super::*;

    /// The counter is process-wide, so the tests that read it take turns.
    static COUNTER_LOCK: Mutex<()> = Mutex::new(());

    /// Enough rows to cover every kind without filling the 10,000-row scrollback.
    const TEST_ROWS: usize = 100;

    #[test]
    fn memory_kinds_are_full_rows() {
        for kind in KINDS {
            let mut term = Terminal::new(
                Size {
                    rows: LINES as u16,
                    cols: COLUMNS as u16,
                },
                Config::default(),
            );
            let mut batch = EventBatch::new();
            term.feed(&(kind.make)(), &mut batch, Instant::now());
            assert_eq!(term.lines_produced(), 1, "{}", kind.name);
            let screen = term.screen();
            assert!(
                !screen.row(screen.row_of_index(0)).wrapped(),
                "{}",
                kind.name
            );
            assert_eq!(screen.cursor_row_index(), 1, "{}", kind.name);
            assert_eq!(screen.cursor().pos.col, 0, "{}", kind.name);
        }
    }

    #[test]
    fn counting_allocator_tracks_a_vec() {
        let _turn = COUNTER_LOCK.lock().unwrap_or_else(PoisonError::into_inner);
        let before = live_heap_bytes();
        let held = Vec::<u8>::with_capacity(64 << 20);
        let during = live_heap_bytes();
        assert!(during >= before + (48 << 20), "{before} -> {during}");
        drop(held);
        let after = live_heap_bytes();
        assert!(during >= after + (48 << 20), "{during} -> {after}");
    }

    #[test]
    fn memory_costs_are_per_kind_and_per_row() {
        let _turn = COUNTER_LOCK.lock().unwrap_or_else(PoisonError::into_inner);
        let rows = measure_rows(TEST_ROWS);
        let names: Vec<&str> = rows.iter().map(|row| row.content.as_str()).collect();
        assert_eq!(names, ["plain", "unicode", "styled", "mixed"]);
        for row in &rows {
            // Other tests allocate and free concurrently, so only the arithmetic is exact.
            let expected = row.heap_bytes as f64 / TEST_ROWS as f64;
            assert!((row.heap_bytes_per_row - expected).abs() < 1e-9);
        }
        let table = render_table(&rows);
        assert!(table.starts_with("| Content | heap bytes | bytes/row |\n|---|---:|---:|\n"));
        assert_eq!(table.lines().count(), 2 + rows.len());
    }
}
