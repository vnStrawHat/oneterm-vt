# Release v0.1.0: checklist, script kinds, post-release

## Which version argument (Decision 1)

The workspace version is already `0.1.0` and no tag exists. `scripts/release.sh 0.1.0` uses an
explicit `X.Y.Z` as given: the bump step rewrites the same line, the gate runs, the record and
chart are written, the commit `release: v0.1.0` holds `perf/history/v0.1.0.json` and
`perf/chart.svg`, and the tag is `v0.1.0`. A kind (`minor`) would skip `0.1.0` itself; a
"pre-bump" commit would be churn. The script needs no new flag.

## Script change: kinds under 0.x (Decision 2)

Today `major` on `0.1.0` gives `1.0.0`, which contradicts versioning.md ("a major change goes
`0.x` to `0.(x+1)`"). New mapping, the only code change to `scripts/release.sh`:

| Current | `patch` | `minor` | `major` |
|---|---|---|---|
| `0.x.y` | `0.x.(y+1)` | `0.x.(y+1)` | `0.(x+1).0` |
| `M.x.y`, `M >= 1` | `M.x.(y+1)` | `M.(x+1).0` | `(M+1).0.0` |

```bash
case "$ARG" in
    major) if [ "$MA" -eq 0 ]; then V="0.$((MI + 1)).0"; else V="$((MA + 1)).0.0"; fi ;;
    minor) if [ "$MA" -eq 0 ]; then V="0.$MI.$((PA + 1))"; else V="$MA.$((MI + 1)).0"; fi ;;
    patch) V="$MA.$MI.$((PA + 1))" ;;
    *) V="$ARG" ;;
esac
```

Also: the header comment gains one line naming the 0.x mapping; the two version regexes escape
their dots (`[0-9]+\.[0-9]+\.[0-9]+`; today `0a1b2` passes). Nothing else changes: the script
still never pushes.

## Checklist (in order; owner, Git Bash, the baseline machine)

| # | Step | Pass when |
|---|---|---|
| 1 | Phases 0-13 committed on `main`; `git status` clean | every spec row in `docs/README.md`; every row in `docs/user/README.md` linked |
| 2 | Repository URL decided (open item 1); `git remote add origin <url>` | README "Use" and the `versioning.md` pin carry the URL (part of the phase-13 commit) |
| 3 | `git push origin main`; wait for CI | `check` green on Linux, Windows, macOS: gate, `--no-default-features` + tree check, docs, **public-API snapshot** (Linux, Windows), **headless example** |
| 4 | Public-API review | tests.md M2 recorded in `acceptance.md`; no unlisted root item |
| 5 | Fuzz | `parser`, `feed`, `resize` 10 min each, clean, at this commit (fuzz.md); recorded |
| 6 | Perf verdict | `docs/specs/0013-perf/acceptance.md`: all ten tier-2 fixtures pass or carry a waiver. `vt-bench grid --check` now exits 2 ("no perf record"): expected, no baseline exists yet |
| 7 | Quiet machine, then `VT_BENCH_MACHINE="i7-12700 12C/20T, Windows 11 26200, rustc 1.96.0" scripts/release.sh 0.1.0` | prints `releasing v0.1.0 -> v0.1.0` and finally `next: git push origin main "v0.1.0"` |
| 8 | `cargo run -p oneterm-vt-bench --release -- grid --check`; open `perf/chart.svg` | `PASS: 10 of 10` against the new record (proves it parses and is a usable baseline); chart shows one point per fixture. On FAIL (noisy run): `git tag -d v0.1.0 && git reset --hard HEAD~1`, quiet the machine, redo 7 (nothing is pushed yet) |
| 9 | `git push origin main v0.1.0` | the Release workflow starts |
| 10 | `gh run watch` on the Release run | `verify`, `test` (3 OS), `publish` green |
| 11 | `gh release view v0.1.0` | title `v0.1.0`; notes grouped Features / Bug fixes / Performance, one line per phase commit (`feat(parser): ...` onwards), no `docs`/`ci`/`release` lines; assets `chart.svg` and `v0.1.0.json` |
| 12 | `gh release download v0.1.0 -p '*.json' -D .tmp/release-check` then `cmp` with `perf/history/v0.1.0.json` | identical |
| 13 | Record tag sha, release URL and run ids in `acceptance.md` | done |

Step 7 runs the local gate itself; steps 3-6 exist because CI, fuzzing and the target table
cover what the script does not (nightly snapshot, other OSes, hostile input, target verdict).

## When something fails after the push

Never move, delete or re-push a pushed tag (`docs/agents/commits.md`: no rewriting pushed
history). Fix forward: commit the fix, release `0.1.1` with `scripts/release.sh patch`.
A failed `publish` job alone (no code at fault) is re-run from the Actions page.

## Post-release

- `workspace.package.version` stays `0.1.0` until the next release; there is no "-dev" bump.
  Between releases, `XTVERSION` from `main` still says `0.1.0`; that is accepted.
- Next release: kind = highest bump among commits since `v0.1.0` (commits.md), passed to the
  script, mapped per the table above. `vt-bench grid --check` before it now compares against
  `perf/history/v0.1.0.json` by default.
- `git diff v0.1.0 -- crates/vt/public-api.*.txt` is the surface change of the next release.
