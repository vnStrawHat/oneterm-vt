# The Release workflow (`.github/workflows/release.yml`, replaced whole)

Job graph: `prepare` (ubuntu: release.sh, trip-wire, notes, bundle) -> `test` x3 (the bundled
commit) -> `publish` (push, release; skipped on `dry_run`). The bench runs once. No tag trigger.

```yaml
name: Release
on:
  workflow_dispatch:
    inputs:
      kind: { description: "Bump kind (ignored when version is set)", type: choice, options: [patch, minor, major], default: patch }
      version: { description: "Explicit X.Y.Z; overrides kind", type: string, required: false }
      dry_run: { description: "Do everything except push and release", type: boolean, default: true }
permissions:
  contents: read
concurrency:
  group: release
  cancel-in-progress: false
env:
  CARGO_TERM_COLOR: always
jobs:
  prepare:
    runs-on: ubuntu-latest
    defaults: { run: { shell: bash } }
    outputs:
      tag: ${{ steps.release.outputs.tag }}
    steps:
      - uses: actions/checkout@v4
        with: { fetch-depth: 0 }
      - run: rustup toolchain install
      - uses: Swatinem/rust-cache@v2
      - name: Describe the runner
        run: |
          . /etc/os-release
          cpu="$(LC_ALL=C lscpu | sed -n 's/^Model name: *//p')"
          rustc="$(rustc --version | cut -d ' ' -f 2)"
          echo "VT_BENCH_MACHINE=GitHub ubuntu-latest, $NAME $VERSION_ID, $(nproc) vCPU $cpu, rustc $rustc" >>"$GITHUB_ENV"
      - name: Bump, gate, bench, chart, commit, tag
        id: release
        env: { RELEASE_ARG: "${{ inputs.version || inputs.kind }}" }
        run: |
          git config user.name "github-actions[bot]"
          git config user.email "41898282+github-actions[bot]@users.noreply.github.com"
          scripts/release.sh "$RELEASE_ARG"
          echo "tag=$(git describe --exact-match HEAD)" >>"$GITHUB_OUTPUT"
      - name: Perf trip-wire
        env: { TAG: "${{ steps.release.outputs.tag }}" }
        run: |
          prev="$(find perf/history -name 'v*.json' ! -name "$TAG.json" | sort -V | tail -n 1)"
          cargo run -p oneterm-vt-bench --release -- grid --check ${prev:+--baseline "$prev"} \
            | tee -a "$GITHUB_STEP_SUMMARY"
      - name: Release notes
        uses: orhun/git-cliff-action@v4
        with: { config: cliff.toml, args: --latest --strip header }
        env: { OUTPUT: .tmp/release-notes.md }
      - name: Stage the release
        env: { TAG: "${{ steps.release.outputs.tag }}" }
        run: |
          out="$RUNNER_TEMP/release"
          mkdir -p "$out"
          git bundle create "$out/release.bundle" "refs/tags/$TAG"
          cp .tmp/release-notes.md "$out/notes.md"
          cp perf/chart.svg "perf/history/$TAG.json" "$out/"
          cat "$out/notes.md" >>"$GITHUB_STEP_SUMMARY"
      - uses: actions/upload-artifact@v4
        with: { name: release, path: "${{ runner.temp }}/release", if-no-files-found: error, retention-days: 7 }
  test:
    needs: prepare
    name: test (${{ matrix.os }})
    runs-on: ${{ matrix.os }}
    strategy:
      fail-fast: false
      matrix: { os: [ubuntu-latest, windows-latest, macos-latest] }
    defaults: { run: { shell: bash } }
    env: { TAG: "${{ needs.prepare.outputs.tag }}" }
    steps:
      - uses: actions/checkout@v4
        with: { fetch-depth: 0 }
      - uses: actions/download-artifact@v4
        with: { name: release, path: .tmp/release }
      - name: Check out the release commit
        run: |
          git fetch .tmp/release/release.bundle "refs/tags/$TAG:refs/tags/$TAG"
          git checkout --detach "$TAG"
      - run: rustup toolchain install
      - uses: Swatinem/rust-cache@v2
      - run: cargo build --workspace --release
      - run: cargo test --workspace
  publish:
    needs: [prepare, test]
    if: ${{ !inputs.dry_run }}
    runs-on: ubuntu-latest
    permissions: { contents: write }
    env: { TAG: "${{ needs.prepare.outputs.tag }}", GH_TOKEN: "${{ github.token }}" }
    steps:
      - uses: actions/checkout@v4
        with: { fetch-depth: 0 }
      - uses: actions/download-artifact@v4
        with: { name: release, path: .tmp/release }
      - name: Push the release commit and tag
        run: |
          git fetch .tmp/release/release.bundle "refs/tags/$TAG:refs/tags/$TAG"
          git push --atomic origin "$(git rev-parse "$TAG^{commit}"):refs/heads/main" "refs/tags/$TAG"
      - name: Create the GitHub release
        run: >
          gh release view "$TAG" >/dev/null 2>&1 ||
          gh release create "$TAG" --verify-tag --title "$TAG" --notes-file .tmp/release/notes.md
          .tmp/release/chart.svg ".tmp/release/$TAG.json"
```

## Guards (every one stops the run before anything is pushed)

| Risk | Guard |
|---|---|
| run from another branch | release.sh: `releases are cut from main` (checkout makes a local branch named after the ref) |
| dirty tree, existing tag or record | release.sh checks; `fetch-depth: 0` brings every remote tag |
| two runs at once | `concurrency: release`, not cancelled; the queued run then stops on the existing tag, or its push is refused (next row) |
| `main` moved since dispatch | `git push --atomic` is refused as non-fast-forward; neither ref lands |
| tested commit differs from pushed one | the bundle carries the release commit and its annotated tag; `test` and `publish` fetch the same objects |
| perf collapse | trip-wire step exits 1 (see `docs/perf/README.md`) |
| bad input | release.sh validates `RELEASE_ARG` (usage, exit 2); passed through `env`, never inlined into `run` |

`contents: write` exists only in `publish`; every other job reads. Pushes made with the job token
start no other workflow, so CI does not run on the release commit; `test` replaces it.

Dry run and its check: tests.md M6. Re-running a failed `publish`: release.md "When something fails".
