# GitHub Actions

> Note: `release.yml` is now a manual workflow, see docs/specs/0014-release/workflow.md; the
> `release.yml` section below is history.

Toolchain: `rustup toolchain install` with no argument installs the channel and components that
`rust-toolchain.toml` pins (rustup >= 1.28, current on hosted runners). No toolchain action.
Actions are pinned by major tag; SHA pinning is an open item.

## `.github/workflows/ci.yml`

```yaml
name: CI
on:
  push:
    branches: [main]
  pull_request:
permissions:
  contents: read
concurrency:
  group: ci-${{ github.ref }}
  cancel-in-progress: true
env:
  CARGO_TERM_COLOR: always
jobs:
  check:
    name: check (${{ matrix.os }})
    runs-on: ${{ matrix.os }}
    strategy:
      fail-fast: false
      matrix:
        os: [ubuntu-latest, windows-latest, macos-latest]
    defaults:
      run:
        shell: bash
    steps:
      - uses: actions/checkout@v4
      - name: Install pinned toolchain
        run: rustup toolchain install
      - uses: Swatinem/rust-cache@v2
      - name: Format
        if: runner.os == 'Linux'
        run: cargo fmt --all -- --check
      - name: Shell scripts
        if: runner.os == 'Linux'
        run: shellcheck scripts/*.sh
      - name: Clippy (all features)
        run: cargo clippy --workspace --all-targets --all-features -- -D warnings
      - name: Test
        run: cargo test --workspace
      - name: Test with all features
        if: runner.os == 'Linux'
        run: cargo test --workspace --all-features
      - name: Without default features
        if: runner.os == 'Linux'
        run: |
          cargo clippy -p oneterm-vt --all-targets --no-default-features -- -D warnings
          cargo test -p oneterm-vt --no-default-features
      - name: Docs
        env:
          RUSTDOCFLAGS: -D warnings
        run: |
          cargo doc --workspace --no-deps
          cargo doc --workspace --no-deps --all-features
```

Linux-only steps do not depend on the OS; only `pty` is platform code, and it is on by default everywhere.

## `.github/workflows/release.yml`

```yaml
name: Release
on:
  push:
    tags: ["v*"]
permissions:
  contents: read
env:
  CARGO_TERM_COLOR: always
jobs:
  verify:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - name: Tag, version, changelog and perf record agree
        shell: bash
        run: |
          set -euo pipefail
          version="${GITHUB_REF_NAME#v}"
          declared="$(sed -n '/^\[workspace\.package\]/,/^\[/ s/^version = "\(.*\)"$/\1/p' Cargo.toml)"
          [ "$version" = "$declared" ] || { echo "::error::tag $GITHUB_REF_NAME, workspace version $declared"; exit 1; }
          [ -f "perf/history/v$version.json" ] || { echo "::error::perf/history/v$version.json missing"; exit 1; }
          grep -q "^## \[$version\]" CHANGELOG.md || { echo "::error::CHANGELOG.md has no [$version] heading"; exit 1; }
  test:
    needs: verify
    name: test (${{ matrix.os }})
    runs-on: ${{ matrix.os }}
    strategy:
      fail-fast: false
      matrix:
        os: [ubuntu-latest, windows-latest, macos-latest]
    defaults:
      run:
        shell: bash
    steps:
      - uses: actions/checkout@v4
      - run: rustup toolchain install
      - uses: Swatinem/rust-cache@v2
      - run: cargo build --workspace --release
      - run: cargo test --workspace
  publish:
    needs: test
    runs-on: ubuntu-latest
    permissions:
      contents: write
    steps:
      - uses: actions/checkout@v4
      - name: Create the GitHub release
        env:
          GH_TOKEN: ${{ github.token }}
        run: >
          gh release create "$GITHUB_REF_NAME" --verify-tag --generate-notes
          --title "$GITHUB_REF_NAME"
          perf/chart.svg "perf/history/$GITHUB_REF_NAME.json"
```

The release attaches the chart and the perf record; the crate itself is consumed by git tag.
