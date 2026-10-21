#!/usr/bin/env bash
# SessionStart hook: confine cargo, temp files and Claude to the project folder.
#
# Resolves the project root (the main checkout, even when the session cd's into a
# .claude/worktrees/* worktree) and exports the paths into CLAUDE_ENV_FILE so every
# later Bash call inherits them:
#   CARGO_HOME        -> <root>/.cargo-home   (registry + git caches, one copy per project)
#   CARGO_TARGET_DIR  -> <root>/target        (one build cache shared by all worktrees)
#   TMP/TEMP/TMPDIR   -> <root>/.tmp
#   CARGO_BUILD_JOBS  -> min(cpus, RAM_GB / 3)   (parallel rustc processes; OOM guard)
#
# Without CLAUDE_ENV_FILE the exports go to stdout, so a shell that missed the hook
# can run: eval "$(bash .claude/hooks/cargo-env.sh)"
set -eu

root="${CLAUDE_PROJECT_DIR:-$PWD}"
if common=$(git -C "$root" rev-parse --path-format=absolute --git-common-dir 2>/dev/null); then
  root=$(dirname "$common")
fi
# Git Bash gives /d/foo; cargo and rustc want D:/foo.
if command -v cygpath >/dev/null 2>&1; then
  root=$(cygpath -m "$root")
fi

mkdir -p "$root/.cargo-home" "$root/.tmp" "$root/target"

# Each rustc process can take 2-3G on a heavy crate, and Windows without a pagefile cannot
# exceed physical RAM. Allow one job per 3G of RAM, never more than the CPU count.
cpus=$(nproc 2>/dev/null || echo 4)
mem_kb=$(awk '/MemTotal/ {print $2}' /proc/meminfo 2>/dev/null || echo 0)
jobs=$(( mem_kb / 1024 / 1024 / 3 ))
[ "$jobs" -lt 1 ] && jobs=1
[ "$jobs" -gt "$cpus" ] && jobs=$cpus

{
  echo "export CARGO_HOME=\"$root/.cargo-home\""
  echo "export CARGO_TARGET_DIR=\"$root/target\""
  echo "export TMP=\"$root/.tmp\""
  echo "export TEMP=\"$root/.tmp\""
  echo "export TMPDIR=\"$root/.tmp\""
  echo "export CARGO_BUILD_JOBS=$jobs"
} >> "${CLAUDE_ENV_FILE:-/dev/stdout}"

echo "cargo-env: root=$root (CARGO_HOME, CARGO_TARGET_DIR, TMP set; CARGO_BUILD_JOBS=$jobs)" >&2

# Worktrees and target/ eat disk fast; warn before a build fills the drive.
free_gb=$(( $(df -Pk "$root" | awk 'NR==2 {print $4}') / 1024 / 1024 ))
if [ "$free_gb" -lt 20 ]; then
  echo "cargo-env: WARNING only ${free_gb}G free on this drive; run 'cargo clean -p <crate>' or remove stale worktrees before building more" >&2
fi
