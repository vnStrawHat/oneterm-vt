// @xterm/headless 6.0.0 behind the comparison harness (docs/specs/0016-cross-engine-bench).
//
// Same command line and output as the Rust adapters (compare/harness/src/cli.rs):
//   node --expose-gc adapter.mjs info
//   node --expose-gc adapter.mjs grid   --file F [--passes 5] [--warmup 1] [common]
//   node --expose-gc adapter.mjs verify --file F [--checkpoints 4]          [common]
//   node adapter.mjs selftest            canonical-dump cases shared with the Rust harness (X1)
// common: [--cols 160] [--rows 45] [--scrollback 10000] [--chunk 65536]
//
// Chunks go to the engine through the internal `_core._writeBuffer.writeSync`, so the figure is
// engine work and not event-loop yields between the public `write` slices (spec decision D4).
// The pinned 6.0.0 keeps that internal stable.

import { createHash } from 'node:crypto';
import { readFileSync } from 'node:fs';
import { createRequire } from 'node:module';
import { fileURLToPath } from 'node:url';

const require = createRequire(import.meta.url);

// ---------------------------------------------------------------------------------------------
// Canonical dump: a copy of compare/harness/src/dump.rs. Colours and flags arrive already
// encoded ("d", "i<n>", "r<rrggbb>"; flags as the sorted letters "biru").
// ---------------------------------------------------------------------------------------------

class RowCells {
  constructor(trackRuns) {
    this.trackRuns = trackRuns;
    this.column = 0;
    this.text = '';
    this.lead = { fg: 'd', bg: 'd', flags: '', uri: null };
    this.attrStart = null; // { start, fg, bg, flags }
    this.linkStart = null; // { start, uri }
    this.attrs = [];
    this.links = [];
  }

  cell(c) {
    if (c.spacer) {
      this.advance();
    } else {
      this.text += c.text === '' ? ' ' : c.text;
      if (this.trackRuns) this.lead = { fg: c.fg, bg: c.bg, flags: c.flags, uri: c.uri ?? null };
      this.advance();
    }
    this.column += 1;
  }

  advance() {
    if (!this.trackRuns) return;
    const { fg, bg, flags, uri } = this.lead;
    const open = this.attrStart;
    const same = open !== null && open.fg === fg && open.bg === bg && open.flags === flags;
    if (!same) {
      this.closeAttr(this.column);
      if (!(fg === 'd' && bg === 'd' && flags === '')) {
        this.attrStart = { start: this.column, fg, bg, flags };
      }
    }
    const sameLink = this.linkStart === null ? uri === null : uri !== null && this.linkStart.uri === uri;
    if (!sameLink) {
      this.closeLink(this.column);
      if (uri !== null) this.linkStart = { start: this.column, uri };
    }
  }

  closeAttr(end) {
    if (this.attrStart === null) return;
    const { start, fg, bg, flags } = this.attrStart;
    this.attrs.push([start, end, fg, bg, flags]);
    this.attrStart = null;
  }

  closeLink(end) {
    if (this.linkStart === null) return;
    this.links.push([this.linkStart.start, end, this.linkStart.uri]);
    this.linkStart = null;
  }

  finish() {
    this.closeAttr(this.column);
    this.closeLink(this.column);
    return { text: this.text.replace(/ +$/, ''), attrs: this.attrs, links: this.links };
  }
}

class DumpBuilder {
  constructor(columns, wantsHistory) {
    this.columns = columns;
    this.history_wanted = wantsHistory;
    this.cursorAt = [0, 0];
    this.rows = [];
    this.history = [];
    this.attrs = [];
    this.links = [];
  }

  wantsHistory() {
    return this.history_wanted;
  }

  cursor(row, col) {
    this.cursorAt = [row, Math.min(col, Math.max(this.columns - 1, 0))];
  }

  row(fill) {
    const row = this.rows.length;
    const cells = new RowCells(true);
    fill(cells);
    const done = cells.finish();
    this.rows.push(done.text);
    for (const [a, b, fg, bg, flags] of done.attrs) this.attrs.push([row, a, b, fg, bg, flags]);
    for (const [a, b, uri] of done.links) this.links.push([row, a, b, uri]);
  }

  historyRow(fill) {
    if (!this.history_wanted) return;
    const cells = new RowCells(false);
    fill(cells);
    this.history.push(cells.finish().text);
  }

  finish() {
    // Key order is the canonical JSON order.
    return {
      cursor: this.cursorAt,
      rows: this.rows,
      history: this.history,
      attrs: this.attrs,
      links: this.links,
    };
  }
}

const sha256 = (text) => createHash('sha256').update(text, 'utf8').digest('hex');
const canonical = (dump) => JSON.stringify(dump);

// ---------------------------------------------------------------------------------------------
// The engine.
// ---------------------------------------------------------------------------------------------

const { Terminal } = require('@xterm/headless');
const { UnicodeGraphemesAddon } = require('@xterm/addon-unicode-graphemes');

const SGR_MODE_DEFAULT = 0;
const SGR_MODE_RGB = 0x3000000;

function createTerminal(geometry) {
  const term = new Terminal({
    cols: geometry.cols,
    rows: geometry.rows,
    scrollback: geometry.scrollback,
    allowProposedApi: true,
  });
  // Grapheme clustering is switched on in-band by the fixture (`?2027h`), as for every engine.
  term.loadAddon(new UnicodeGraphemesAddon());
  term.unicode.activeVersion = '15-graphemes';
  return term;
}

function capabilities() {
  const probe = new Terminal({ cols: 2, rows: 1, allowProposedApi: true });
  const found = [];
  try {
    probe.loadAddon(new UnicodeGraphemesAddon());
    probe.unicode.activeVersion = '15-graphemes';
    found.push('graphemes');
  } catch {
    // The addon does not load headless: the capability is not declared.
  }
  if (typeof probe._core?._oscLinkService?.getLinkData === 'function') found.push('hyperlinks');
  return found;
}

function info() {
  return {
    name: 'xterm',
    version: '6.0.0',
    source: `npm @xterm/headless@6.0.0 + @xterm/addon-unicode-graphemes@0.4.0, Node ${process.version}`,
    language: 'javascript (V8)',
    build: 'node --expose-gc, writeSync feed',
    capabilities: capabilities(),
    labels: ['rss', '+graphemes addon', 'JIT/GC on pinned core'],
  };
}

function colour(mode, value) {
  if (mode === SGR_MODE_DEFAULT) return 'd';
  if (mode === SGR_MODE_RGB) return `r${(value & 0xffffff).toString(16).padStart(6, '0')}`;
  return `i${value}`;
}

function flagsOf(cell) {
  return (cell.isBold() ? 'b' : '') + (cell.isItalic() ? 'i' : '') + (cell.isInverse() ? 'r' : '') + (cell.isUnderline() ? 'u' : '');
}

function pushLine(term, line, geometry, out) {
  const links = term._core._oscLinkService;
  for (let x = 0; x < geometry.cols; x++) {
    const cell = line.getCell(x);
    if (!cell) {
      out.cell({ text: '', wide: false, spacer: false, fg: 'd', bg: 'd', flags: '', uri: null });
      continue;
    }
    const width = cell.getWidth();
    const urlId = cell.extended?.urlId ?? 0;
    out.cell({
      text: cell.getChars(),
      wide: width === 2,
      spacer: width === 0,
      fg: colour(cell.getFgColorMode(), cell.getFgColor()),
      bg: colour(cell.getBgColorMode(), cell.getBgColor()),
      flags: flagsOf(cell),
      uri: urlId ? (links.getLinkData(urlId)?.uri ?? null) : null,
    });
  }
}

function dump(term, geometry, wantsHistory) {
  const builder = new DumpBuilder(geometry.cols, wantsHistory);
  const active = term.buffer.active;
  builder.cursor(active.cursorY, active.cursorX);
  if (builder.wantsHistory()) {
    // The primary buffer's history, even while the alternate screen is shown.
    const primary = term.buffer.normal;
    const above = primary.baseY;
    for (let y = Math.max(0, above - geometry.rows); y < above; y++) {
      builder.historyRow((row) => pushLine(term, primary.getLine(y), geometry, row));
    }
  }
  for (let y = 0; y < geometry.rows; y++) {
    builder.row((row) => pushLine(term, active.getLine(active.viewportY + y), geometry, row));
  }
  return builder.finish();
}

const feed = (term, bytes) => term._core._writeBuffer.writeSync(bytes);

// ---------------------------------------------------------------------------------------------
// Commands.
// ---------------------------------------------------------------------------------------------

function checkpointChunks(chunks, count) {
  const marks = [];
  for (let q = 1; q <= count; q++) marks.push(Math.max(Math.ceil((chunks * q) / count), 1));
  return marks;
}

function privateBytes() {
  global.gc?.();
  return process.memoryUsage().rss;
}

function runVerify(bytes, geometry, chunk, checkpoints) {
  const base = privateBytes();
  const term = createTerminal(geometry);
  const chunks = Math.ceil(bytes.length / chunk);
  const marks = checkpointChunks(chunks, checkpoints);
  const dumps = [];
  let fed = 0;
  let steady = 0;
  marks.forEach((mark, index) => {
    const upto = Math.min(mark, chunks);
    for (; fed < upto; fed++) feed(term, bytes.subarray(fed * chunk, (fed + 1) * chunk));
    const last = index + 1 === marks.length;
    if (last) steady = privateBytes();
    dumps.push(dump(term, geometry, last));
  });
  const peak = (process.resourceUsage().maxRSS ?? 0) * 1024;
  return {
    dumps,
    final_sha256: sha256(canonical(dumps[dumps.length - 1])),
    memory: { base, steady, peak },
    scrollback_rows: null,
  };
}

function runGrid(bytes, geometry, chunk, passes, warmup) {
  const samples = [];
  let finalHash = '';
  const total = warmup + passes;
  for (let pass = 0; pass < total; pass++) {
    let term = createTerminal(geometry);
    const start = process.hrtime.bigint();
    for (let at = 0; at < bytes.length; at += chunk) feed(term, bytes.subarray(at, at + chunk));
    const elapsed = process.hrtime.bigint() - start;
    if (pass >= warmup) samples.push(Number(elapsed));
    if (pass + 1 === total) finalHash = sha256(canonical(dump(term, geometry, true)));
    term = null;
  }
  return { bytes: bytes.length, samples_ns: samples, final_sha256: finalHash };
}

function selftest() {
  const path = fileURLToPath(new URL('../../harness/tests/dump_cases.json', import.meta.url));
  const cases = JSON.parse(readFileSync(path, 'utf8'));
  let failed = 0;
  for (const c of cases) {
    const builder = new DumpBuilder(c.cols, c.history);
    builder.cursor(c.cursor[0], c.cursor[1]);
    const push = (row, cells) => {
      for (const [text, wide, spacer, fg, bg, flags, uri] of cells) {
        row.cell({ text, wide: wide === 1, spacer: spacer === 1, fg, bg, flags, uri });
      }
    };
    for (const cells of c.history_rows) builder.historyRow((row) => push(row, cells));
    for (const cells of c.rows) builder.row((row) => push(row, cells));
    const got = canonical(builder.finish());
    if (got !== c.expected) {
      failed += 1;
      console.error(`FAIL ${c.name}\n  expected ${c.expected}\n  got      ${got}`);
    }
  }
  return failed === 0 ? { selftest: 'ok', cases: cases.length } : null;
}

// ---------------------------------------------------------------------------------------------
// Command line (the rules of compare/harness/src/cli.rs).
// ---------------------------------------------------------------------------------------------

function usage(message) {
  console.error(`engine-xterm: ${message}`);
  process.exit(2);
}

function parseArgs(args) {
  const [command, ...flags] = args;
  if (!['info', 'grid', 'verify', 'selftest'].includes(command ?? '')) {
    usage(command ? `unknown command '${command}'` : 'missing command: info | grid | verify');
  }
  const options = { cols: 160, rows: 45, scrollback: 10000, chunk: 65536, passes: 5, warmup: 1, checkpoints: 4, file: null };
  const allowed = {
    '--cols': () => true,
    '--rows': () => true,
    '--scrollback': () => true,
    '--chunk': () => true,
    '--file': () => command === 'grid' || command === 'verify',
    '--passes': () => command === 'grid',
    '--warmup': () => command === 'grid',
    '--checkpoints': () => command === 'verify',
  };
  for (let i = 0; i < flags.length; i += 2) {
    const flag = flags[i];
    if (!allowed[flag]?.()) usage(`unknown flag '${flag}' for '${command}'`);
    const value = flags[i + 1];
    if (value === undefined) usage(`flag '${flag}' needs a value`);
    const key = flag.slice(2);
    if (key === 'file') {
      options.file = value;
      continue;
    }
    const number = Number(value);
    if (!Number.isInteger(number) || number < (key === 'warmup' ? 0 : 1)) {
      usage(`${flag} needs a whole number, got '${value}'`);
    }
    options[key] = number;
  }
  if ((command === 'grid' || command === 'verify') && options.file === null) usage(`${command} needs --file F`);
  return { command, options };
}

function main() {
  const { command, options } = parseArgs(process.argv.slice(2));
  const geometry = { cols: options.cols, rows: options.rows, scrollback: options.scrollback };
  let result;
  try {
    if (command === 'info') {
      result = info();
    } else if (command === 'selftest') {
      result = selftest();
    } else {
      const bytes = readFileSync(options.file);
      result =
        command === 'grid'
          ? runGrid(bytes, geometry, options.chunk, options.passes, options.warmup)
          : runVerify(bytes, geometry, options.chunk, options.checkpoints);
    }
  } catch (error) {
    console.error(`engine-xterm: ${error.message}`);
    process.exit(1);
  }
  if (result === null) process.exit(1);
  process.stdout.write(`${JSON.stringify(result)}\n`);
}

main();
