# The Release workflow (`.github/workflows/release.yml`, replaced whole)

Longer than 120 lines on purpose: it holds the YAML for verbatim copy.

Job graph (13g):

- `bump` (ubuntu): `release.sh bump` = guards, version, gate, commit `release: vX` (Cargo files only); bundle
- `bench` x3 (linux, windows, macos): that commit; `release.sh bench` = test, trip-wire, record; upload the record
- `finalize` (ubuntu): three records in, `release.sh commit` = three charts, amend the commit, tag; notes; bundle
- `publish` (ubuntu, skipped on `dry_run`): push commit and tag atomically, GitHub release with 6 assets

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
  bump:
    runs-on: ubuntu-latest
    defaults: { run: { shell: bash } }
    outputs:
      tag: ${{ steps.bump.outputs.tag }}
    steps:
      - uses: actions/checkout@v4
        with: { fetch-depth: 0 }
      - run: rustup toolchain install
      - uses: Swatinem/rust-cache@v2
      - name: Bump, gate, commit
        id: bump
        env: { RELEASE_ARG: "${{ inputs.version || inputs.kind }}" }
        run: |
          git config user.name "github-actions[bot]"
          git config user.email "41898282+github-actions[bot]@users.noreply.github.com"
          bash scripts/release.sh bump "$RELEASE_ARG"
          echo "tag=$(git log -1 --format=%s | cut -d ' ' -f 2)" >>"$GITHUB_OUTPUT"
          mkdir -p "$RUNNER_TEMP/bump"
          git bundle create "$RUNNER_TEMP/bump/bump.bundle" refs/heads/main
      - uses: actions/upload-artifact@v4
        with: { name: bump, path: "${{ runner.temp }}/bump", if-no-files-found: error, retention-days: 7, overwrite: true }
  bench:
    needs: bump
    name: bench (${{ matrix.platform }})
    runs-on: ${{ matrix.os }}
    strategy:
      fail-fast: false
      matrix:
        include:
          - { os: ubuntu-latest, platform: linux }
          - { os: windows-latest, platform: windows }
          - { os: macos-latest, platform: macos }
    defaults: { run: { shell: bash } }
    steps:
      - uses: actions/checkout@v4
        with: { fetch-depth: 0 }
      - uses: actions/download-artifact@v4
        with: { name: bump, path: .tmp/bump }
      - name: Check out the bump commit
        run: |
          git fetch .tmp/bump/bump.bundle main
          git checkout --detach FETCH_HEAD
      - run: rustup toolchain install
      - uses: Swatinem/rust-cache@v2
      - name: Describe the runner
        env: { RUNNER_LABEL: "${{ matrix.os }}" }
        run: |
          case "$RUNNER_OS" in
            Linux)
              . /etc/os-release
              os="$NAME $VERSION_ID"
              cpu="$(LC_ALL=C lscpu | sed -n 's/^Model name: *//p')"
              cores="$(nproc)" ;;
            Windows)
              os="$(pwsh -NoProfile -Command '$o = Get-CimInstance Win32_OperatingSystem; "$($o.Caption) $($o.Version)"' | tr -d '\r')"
              cpu="$(pwsh -NoProfile -Command '(Get-CimInstance Win32_Processor | Select-Object -First 1).Name.Trim()' | tr -d '\r')"
              cores="$(nproc)" ;;
            macOS)
              os="$(sw_vers -productName) $(sw_vers -productVersion)"
              cpu="$(sysctl -n machdep.cpu.brand_string)"
              cores="$(sysctl -n hw.ncpu)" ;;
            *) echo "::error::no runner description for $RUNNER_OS"; exit 1 ;;
          esac
          rustc="$(rustc --version | cut -d ' ' -f 2)"
          echo "VT_BENCH_MACHINE=GitHub $RUNNER_LABEL, $os, $cores vCPU $cpu, rustc $rustc" >>"$GITHUB_ENV"
      - name: Test, trip-wire, record
        run: bash scripts/release.sh bench
      - uses: actions/upload-artifact@v4
        with:
          name: record-${{ matrix.platform }}
          path: perf/history/${{ matrix.platform }}/${{ needs.bump.outputs.tag }}.json
          if-no-files-found: error
          retention-days: 7
          overwrite: true
  finalize:
    needs: [bump, bench]
    runs-on: ubuntu-latest
    defaults: { run: { shell: bash } }
    env: { TAG: "${{ needs.bump.outputs.tag }}" }
    steps:
      - uses: actions/checkout@v4
        with: { fetch-depth: 0 }
      - uses: actions/download-artifact@v4
        with: { name: bump, path: .tmp/bump }
      - uses: actions/download-artifact@v4
        with: { pattern: "record-*", path: .tmp/records }
      - name: Check out the bump commit, add the records
        run: |
          git fetch .tmp/bump/bump.bundle main
          git checkout --detach FETCH_HEAD
          for platform in linux windows macos; do
            mkdir -p "perf/history/$platform"
            cp ".tmp/records/record-$platform/$TAG.json" "perf/history/$platform/"
          done
      - run: rustup toolchain install
      - uses: Swatinem/rust-cache@v2
      - name: Chart, amend, tag
        run: |
          git config user.name "github-actions[bot]"
          git config user.email "41898282+github-actions[bot]@users.noreply.github.com"
          bash scripts/release.sh commit
      - name: Release notes
        uses: orhun/git-cliff-action@v4
        with: { config: cliff.toml, args: --latest --strip header }
        env: { OUTPUT: .tmp/release-notes.md }
      - name: Stage the release
        run: |
          out="$RUNNER_TEMP/release"
          mkdir -p "$out"
          git bundle create "$out/release.bundle" "refs/tags/$TAG"
          cp .tmp/release-notes.md "$out/notes.md"
          for platform in linux windows macos; do
            cp "perf/chart-$platform.svg" "$out/"
            cp "perf/history/$platform/$TAG.json" "$out/$TAG-$platform.json"
          done
          cat "$out/notes.md" >>"$GITHUB_STEP_SUMMARY"
      - uses: actions/upload-artifact@v4
        with: { name: release, path: "${{ runner.temp }}/release", if-no-files-found: error, retention-days: 7, overwrite: true }
  publish:
    needs: [bump, finalize]
    if: ${{ !inputs.dry_run }}
    runs-on: ubuntu-latest
    defaults: { run: { shell: bash } }
    permissions: { contents: write }
    env: { TAG: "${{ needs.bump.outputs.tag }}", GH_TOKEN: "${{ github.token }}" }
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
          .tmp/release/chart-*.svg ".tmp/release/$TAG"-*.json
```

Notes for the coder: `run` steps use `shell: bash`, which GitHub runs as `bash -eo pipefail`
(workflow syntax, `defaults.run.shell`), so a failing `grid --check` inside a `| tee` still fails
the step. Without `merge-multiple`, `download-artifact` puts each `record-*` artifact in its own
folder named after it (`.tmp/records/record-linux/v0.1.0.json`). `.tmp/` is git-ignored, so
the artifacts never dirty the tree.

## Guards (every one stops the run before anything is pushed)

| Risk | Guard |
|---|---|
| run from another branch | `release.sh bump`: `releases are cut from main` |
| dirty tree, existing tag or record | `release.sh bump` checks (all three platform folders); `fetch-depth: 0` brings every remote tag |
| two runs at once | `concurrency: release`, not cancelled; the queued run stops on the existing tag, or its push is refused |
| `main` moved since dispatch | `git push --atomic` is refused as non-fast-forward; neither ref lands |
| tested tree differs from pushed one | `bump.bundle` carries the one bump commit to all four later jobs; `release.sh commit` amends only `perf/` and refuses any other change; `release.bundle` carries the tag to `publish` |
| perf collapse on any platform | that `bench` job's trip-wire exits non-zero; `finalize` is skipped, nothing is tagged |
| record filed under the wrong platform | `release.sh bench` takes the folder from `RUNNER_OS`, the upload path from the matrix label (mismatch: no file, upload fails); `vt-bench` refuses a foreign baseline and a mixed chart (platforms.md) |
| bad input | `release.sh` validates sub-command and argument (usage, exit 2); input passed through `env`, never inlined into `run` |

`contents: write` exists only in `publish`. Pushes made with the job token start no other
workflow, so CI does not run on the release commit; `bench` x3 replaces it.

Dry run and its check: tests.md M6. Re-running a failed `publish`: release.md "When something fails".
