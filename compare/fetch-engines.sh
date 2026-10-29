#!/usr/bin/env bash
# Fetches the engine sources the comparison builds (docs/specs/0016-cross-engine-bench/engines.md):
# ghostty (libghostty-vt, with Zig), wezterm and libvterm. Everything lands under
# .cargo-home/compare/ and .tmp/compare/ inside the project. Idempotent: what already exists at the
# pinned commit is skipped. Needs network (not 00:00-05:10 on the owner's machine).
set -euo pipefail

# --- pins: bumping one is a one-line change plus new verify/golden runs ---
GHOSTTY_URL=https://github.com/ghostty-org/ghostty.git
GHOSTTY_COMMIT=b2c78c9bd9bc88a16d9c3769b0fb17ddde1b78b1
WEZTERM_URL=https://github.com/wezterm/wezterm.git
WEZTERM_COMMIT=e68a8ec27727fbbda15e07f0a0737cd5e16481f6
LIBVTERM_URL=https://github.com/neovim/libvterm.git
LIBVTERM_COMMIT=934bc2fbf21800ac3458a499df8820ca5fb45fd3
ZIG_VERSION=0.17.0
ZIG_ARCHIVE=zig-x86_64-windows-0.17.0.zip
ZIG_URL=https://ziglang.org/download/0.17.0/zig-x86_64-windows-0.17.0.zip
ZIG_SHA256=b5663f69581dcf391293fbf16c06cb80d81d806545ce618b4d0bab7f0eb8c428

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CACHE="$ROOT/.cargo-home/compare"
SRC="$CACHE/src"
TOOLS="$CACHE/tools"
LIBGHOSTTY="$CACHE/libghostty-vt"
WORK="$ROOT/.tmp/compare"
mkdir -p "$SRC" "$TOOLS" "$WORK"

native() { if command -v cygpath >/dev/null; then cygpath -m "$1"; else echo "$1"; fi; }
log() { printf '[fetch-engines] %s\n' "$*"; }

# clone_pinned NAME URL COMMIT: blobless clone, detached checkout, no submodules, byte-exact files.
clone_pinned() {
  local name=$1 url=$2 commit=$3 dir="$SRC/$1"
  if [ -d "$dir/.git" ] && [ "$(git -C "$dir" rev-parse HEAD 2>/dev/null)" = "$commit" ]; then
    log "$name already at ${commit:0:9}"
    return
  fi
  if [ ! -d "$dir/.git" ]; then
    git -c core.autocrlf=false clone --filter=blob:none --no-checkout "$url" "$dir"
  fi
  git -C "$dir" -c core.autocrlf=false checkout --detach "$commit"
  log "$name checked out at ${commit:0:9}"
}

fetch_zig() {
  local dir="$TOOLS/zig"
  if [ -x "$dir/zig.exe" ] && [ "$("$dir/zig.exe" version)" = "$ZIG_VERSION" ]; then
    log "zig $ZIG_VERSION already present"
    return
  fi
  rm -rf "$dir" "$TOOLS/zig-unpack"
  mkdir -p "$TOOLS/zig-unpack"
  curl -fL --retry 3 -o "$TOOLS/$ZIG_ARCHIVE" "$ZIG_URL"
  echo "$ZIG_SHA256  $TOOLS/$ZIG_ARCHIVE" | sha256sum -c -
  unzip -q "$TOOLS/$ZIG_ARCHIVE" -d "$TOOLS/zig-unpack"
  mv "$TOOLS/zig-unpack/${ZIG_ARCHIVE%.zip}" "$dir"
  rm -rf "$TOOLS/zig-unpack" "$TOOLS/$ZIG_ARCHIVE"
  log "zig $ZIG_VERSION unpacked"
}

# The corporate proxy on the owner's machine answers Zig's own HTTP client with
# "400 Bad Request" (curl and git work). So: run the build, take every URL it failed to fetch,
# download it with curl (tarballs) or git (git+https), hand the result to `zig fetch`, and
# repeat until the build stops failing on fetches. 9 packages at the pinned commit.
zig_fetch_url() {
  local url=$1 tmp="$WORK/zig-fetch" name
  rm -rf "$tmp" && mkdir -p "$tmp"
  case "$url" in
    git+https://*)
      local repo=${url#git+} commit=${url##*#}
      repo=${repo%%#*}
      git -c core.autocrlf=false clone --filter=blob:none --no-checkout "$repo" "$tmp/repo"
      git -C "$tmp/repo" -c core.autocrlf=false checkout --detach "$commit"
      rm -rf "$tmp/repo/.git"
      "$ZIG" fetch "$(native "$tmp/repo")"
      ;;
    *)
      # A tarball is handed over as its unpacked directory: `zig fetch <archive>` caches only
      # the archive, and `zig build` then derives a nameless hash from it and rejects it.
      name=$(basename "${url%%\?*}")
      curl -fL --retry 3 -o "$tmp/$name" "$url"
      mkdir "$tmp/unpacked"
      tar -xf "$tmp/$name" -C "$tmp/unpacked"
      local root="$tmp/unpacked" entries
      entries=("$root"/*)
      if [ "${#entries[@]}" = 1 ] && [ -d "${entries[0]}" ]; then root=${entries[0]}; fi
      "$ZIG" fetch "$(native "$root")"
      ;;
  esac
}

build_libghostty() {
  local dir="$SRC/ghostty" stamp="$LIBGHOSTTY/.commit"
  if [ -f "$LIBGHOSTTY/lib/ghostty-vt-static.lib" ] && [ "$(cat "$stamp" 2>/dev/null)" = "$GHOSTTY_COMMIT" ]; then
    log "libghostty-vt already built at ${GHOSTTY_COMMIT:0:9}"
    return
  fi
  export ZIG_GLOBAL_CACHE_DIR="$(native "$ROOT/.cargo-home/zig")"
  export ZIG_LOCAL_CACHE_DIR="$(native "$WORK/zig-local")"
  mkdir -p "$ZIG_GLOBAL_CACHE_DIR" "$ZIG_LOCAL_CACHE_DIR"
  local out="$WORK/zig-build.log" attempt urls
  for attempt in $(seq 1 30); do
    log "zig build (attempt $attempt)"
    if (cd "$dir" && "$ZIG" build -Demit-lib-vt=true -Doptimize=ReleaseFast \
        -Dtarget=x86_64-windows-msvc -Dcpu=baseline \
        --prefix "$(native "$LIBGHOSTTY")") >"$out" 2>&1; then
      echo "$GHOSTTY_COMMIT" >"$stamp"
      log "libghostty-vt built"
      return
    fi
    # Zig prints the package it could not fetch as `.url = "..."` or `url: ...`; take both.
    urls=$(grep -Eo '(git\+)?https://[^" ]+' "$out" | sort -u || true)
    if [ -z "$urls" ]; then
      cat "$out" >&2
      log "zig build failed for a reason other than a fetch"
      return 1
    fi
    local fetched=0 url
    for url in $urls; do
      if zig_fetch_url "$url"; then
        fetched=1
      else
        log "could not fetch $url"
      fi
    done
    if [ "$fetched" = 0 ]; then
      cat "$out" >&2
      log "no package could be fetched"
      return 1
    fi
  done
  log "zig build still failing after 30 attempts"
  return 1
}

clone_pinned wezterm "$WEZTERM_URL" "$WEZTERM_COMMIT"
clone_pinned libvterm "$LIBVTERM_URL" "$LIBVTERM_COMMIT"
clone_pinned ghostty "$GHOSTTY_URL" "$GHOSTTY_COMMIT"
fetch_zig
ZIG="$TOOLS/zig/zig.exe"
build_libghostty
log "done: sources in $SRC, libghostty-vt in $LIBGHOSTTY"
