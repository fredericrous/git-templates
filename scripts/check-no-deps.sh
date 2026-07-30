#!/bin/sh
# The hook binary must depend on nothing outside this workspace.
#
# NOT for the reason this comment used to give. It claimed the migration existed
# because a Python repo needed node and zsh installed to commit, so a dependency
# tree "would undo it". That conflates two different things: node and zsh were
# RUNTIME dependencies that had to exist on PATH in every repo, while a Rust
# crate is compiled in and statically linked. Adding `serde` would not make
# anyone install anything. The old justification did not hold.
#
# The reasons that do:
#
#   1. SUPPLY CHAIN, and this is the strong one. This binary runs on every
#      commit in 96 repos, with the developer's credentials, reading every
#      staged file, while nobody is watching. Every transitive crate is code
#      executing in that position. Zero dependencies means that code is std and
#      ours. The argument is about THIS binary; githooks-fleet pulls ratatui and
#      a dozen crates quite happily, because it is opt-in and runs when asked.
#
#   2. OFFLINE REPRODUCIBILITY. A std-only crate builds indefinitely without a
#      registry. Real, though modest.
#
#   3. A FORCING FUNCTION. The guard prevents no specific harm so much as it
#      makes each dependency in the commit path an argued decision rather than a
#      default. It is something to argue WITH, not a wall: if the commit path
#      ever genuinely needs a parser, weigh that crate's tree against the code
#      it replaces and decide. Do not treat this file as a prohibition.
#
# A dependency reaches the commit path through githooks-runtime as easily as
# directly, so the check is on the RESOLVED tree rather than on a manifest.
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
