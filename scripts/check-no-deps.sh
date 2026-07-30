#!/bin/sh
# The hook binary must depend on nothing outside this workspace.
#
# This is the guard that makes the workspace split safe rather than merely
# tidy. `githooks` runs on every commit in 96 repos, and the entire Rust
# migration existed because a Python repo needed node and zsh installed just to
# commit. A dependency reaches the commit path through githooks-runtime as
# easily as directly, so the check is on the RESOLVED tree, not on a manifest.
#
# The fleet TUI depends on the same runtime crate. Its ratatui/crossterm tree
# belongs to githooks-fleet alone, and this is what keeps it there.
set -eu

ext=$(cargo tree -p githooks --edges normal --prefix none 2>/dev/null \
      | sed 's/ (\*)$//' | awk '{print $1}' | sort -u \
      | grep -vE '^(githooks|githooks-runtime)$' || true)

if [ -n "$ext" ]; then
  echo "githooks gained external dependencies:" >&2
  echo "$ext" | sed 's/^/  /' >&2
  echo "" >&2
  echo "The commit path must stay dependency-free. Put this in githooks-fleet." >&2
  exit 1
fi
echo "githooks: no external dependencies"
