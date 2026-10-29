# Criteria: layer, metrics, statistics, knobs, machine

## Layers (this spec: grid + memory; parser-only and resize are "Later", below)

| Layer | What is measured | Status |
|---|---|---|
| **grid** (primary) | bytes in 64 KiB chunks through the engine's public "feed" call into full terminal state (grid, scrollback, modes, styles, links, images it supports); no rendering, no PTY | timed; verified (verification.md) |
| memory | private bytes after the verify pass | informational (`unverified`) |

End-to-end (PTY read, rendering) is **excluded**: it measures the host (ConPTY, renderer,
frame pacing), not the engine, and is where "reads fast, emulates late" numbers come from.

## Metrics

| Metric | Definition |
|---|---|
| `mib_per_second` | input bytes / median pass time, MiB = 2^20 |
| `ns_per_byte` | median pass time / input bytes |
| `spread_percent` | (p90 - p10) / median x 100 over the 15 samples |
| `ratio_to_oneterm` | engine MiB/s / oneterm-vt MiB/s, same fixture; only when both are `ok` or `accepted` |
| `steady_mib` | private bytes sampled after the last feed + `finish` of the verify pass and **before** the dump is built, minus the value taken after the corpus file was loaded and before the terminal was created |
| `peak_mib` | peak private bytes of the verify process minus the same base |

Markers: `≈` tie (difference smaller than the larger spread of the two figures), `†`
accepted divergence, `≠` diverged, `?` unverified, `n/a` unsupported, `fail`.

Summary (no per-engine geomeans over different fixture subsets):
- per engine: count of fixtures `ok` / `accepted` / `diverged` / `unverified` / `n/a` / `fail`;
- one geomean table over the **intersection**: the core fixtures on which every engine in
  the run is `ok` or `accepted`; per engine the geomean of `ratio_to_oneterm` over exactly
  those fixtures, with the intersection's size and names printed above it.

## Statistics

- One **process** per (engine, fixture, repetition). 3 repetitions, 1 untimed warm-up pass +
  5 timed passes each: 15 samples per figure. Median of the 15.
- Each pass builds a fresh terminal **before** the timer and drops it **after** it. The timer
  stops when the last feed call returns and the engine's `finish` (flush of deferred work:
  a pending `?2026` buffer in vte 0.15 for alacritty and vt100, an async queue) returns.
- The adapter reads the whole fixture file into memory before the first pass; the file is
  never read inside the timer. Chunks are slices of that buffer, passed through `black_box`.
- Order: repetition `r` runs fixtures in table order and, per fixture, engines in the fixed
  list rotated by `r`, so no engine always runs first or last after another.
- A process that exits non-zero, times out (600 s) or prints malformed JSON: status `failed`.
- Expected wall time of a full run here: 50-80 min. Recorded in the results, not gated.

## Fixed knobs (every engine, every run)

| Knob | Value | Note |
|---|---|---|
| grid | 160 x 45 | same as vt-bench |
| scrollback | 10 000 lines, no byte cap | Ghostty: byte limit removed and line limit set before the first write (engines.md); libvterm: adapter ring (engines.md) |
| chunk | 65 536 bytes | PTY-read sized; Ghostty's own stream bench uses the same |
| fixture size | 32 MiB (whole units, may exceed slightly) | from `vt-bench fixtures --compare --mib 32` |
| grapheme mode | carried in-band by the fixture (`?2027h`) | engines decide; capability, corpus.md |
| replies | DA/DSR answers go to a discarding writer | no PTY |
| events | the engine's no-op listener | title, bell, clipboard dropped |
| CPU target | baseline x86-64 for every engine | Rust default target; Zig `-Dcpu=baseline`; MSVC default; runtime SIMD dispatch inside engines stays |
| build | Rust adapters: release, `lto = "thin"`, `codegen-units = 1`, `opt-level = 3`; Zig `ReleaseFast`; C via `cc` with `/O2` | same rustc for every Rust engine |

## Machine protocol (the agreed machine)

Intel Core i7-12700 (8P+4E, 20 threads), Windows 11, the owner's desktop. No WSL.

| Rule | How |
|---|---|
| one P-core thread | the driver sets its own affinity to logical CPU 4 (mask `0x10`); children inherit it. Node's V8 GC/JIT helper threads share that core with the xterm adapter's main thread (noted in every report) |
| high priority | children spawned with `HIGH_PRIORITY_CLASS` |
| power | the driver switches to "High performance" for the run and restores the previous scheme afterwards (below); on AC is recorded, warning if not |
| quiet | the driver refuses to start while a `cargo.exe` or `rustc.exe` runs: `CreateToolhelp32Snapshot` + `Process32FirstW/NextW`, exe-name compare (`/proc/*/comm` on Linux) |
| network | not needed for `run`; `fetch-engines.sh` needs it (not 00:00-05:10) |

The driver records the machine automatically (output.md); a run without the pin (e.g. on
Linux) is labelled `unpinned` and never compared against a pinned run.

### Power plan switch (driver, `run` only)

- Before the first process: read the active scheme (`powercfg /getactivescheme`), save its
  GUID to `.tmp/compare/power-restore.txt`, then `powercfg /setactive SCHEME_MIN` (the
  built-in High performance alias). Switch fails: refuse to run (exit 2), nothing changed.
- Restore with `powercfg /setactive <saved GUID>`, then delete the file: on normal exit, on any
  error path and on panic (one drop guard in `run`), and on Ctrl-C / console close
  (`SetConsoleCtrlHandler`; the handler restores, then the driver exits 130).
- A killed driver (no handler runs) leaves the file: every driver start restores from it
  first and says so.
- The record carries `power_scheme_before` and `power_scheme_during` (names and GUIDs).
- `--keep-power-plan` skips the switch and is labelled in the record. Linux: not applicable.

## Later (follow-up spec, not in 0016)

- Parser-only layer: each engine's exposed state machine with a counting no-op sink (ours,
  vte 0.15, rio-vt's parser, vtparse, avt, libvterm; not in Ghostty's C API). Unverifiable,
  so informational only.
- Resize layer: reflow after a scrollback fill (`160x45 -> 100x45 -> 160x45 -> 160x24 ->
  160x45`), gated by goldens like the grid; vt100 has no reflow.
