#!/bin/sh
# Push the shims in templates/hooks/ into every repo's .git/hooks/.
#
# This exists because the sweep has a sharp edge that a hand-typed loop gets
# wrong: the dispatcher collects sub-hooks by GLOB (`<hook>-*`). A repo left
# holding both `pre-commit-ruff.zsh` and `pre-commit-ruff` therefore runs ruff
# TWICE — silently, and only on the repos the loop half-finished. So removal of
# the old name and installation of the new one must both happen, per repo, in
# that order.
#
# Dry-run by default. Nothing is written without --apply.
#
# Not needed for ordinary binary changes: the shims point at one binary, so a
# fix ships to all repos through `make install` alone. Run this only when the
# SHIM SET changes — a hook added, removed, or renamed.

set -eu

ROOT="${ROOT:-$HOME/Developer}"
SRC="$(cd "$(dirname "$0")/.." && pwd)/templates/hooks"
BIN="${GITHOOKS_BIN:-$HOME/.local/bin/githooks}"
APPLY=0
[ "${1:-}" = "--apply" ] && APPLY=1

[ -d "$SRC" ] || { echo "no templates/hooks at $SRC" >&2; exit 1; }
[ -x "$BIN" ] || { echo "githooks binary not found at $BIN — run 'make install' first" >&2; exit 1; }

say() { [ "$APPLY" = "1" ] && echo "$@" || echo "would $@"; }

repos=0 removed=0 installed=0 pkgjson=0 skipped=0 foreign=0

# A file is OURS only if it dispatches to the binary. Anything else in
# .git/hooks is someone's own hook and is never touched.
is_ours() { grep -q -- '--hooks-dir' "$1" 2>/dev/null; }

for gitdir in $(find "$ROOT" -maxdepth 6 -type d -name .git 2>/dev/null | sort); do
  hooks="$gitdir/hooks"
  [ -d "$hooks" ] || continue

  # Update the MANAGED fleet only; never recruit a new repo. Not every .git
  # under $ROOT is source someone commits to by hand: application-landscape
  # creates landscape-history/<id>/<id> as a DATA repo and commits to it
  # programmatically. Seeding hooks there means the app's own commits start
  # running ban-terms and merge-conflict, and a failure breaks the app rather
  # than a developer's commit.
  #
  # "Managed" = at least one shim already dispatches to the binary. Adopting a
  # new repo stays an explicit `git init` against the template.
  managed=0
  for f in "$hooks"/*; do
    [ -f "$f" ] || continue
    if is_ours "$f"; then managed=1; break; fi
  done
  if [ "$managed" = "0" ]; then
    skipped=$((skipped + 1))
    continue
  fi
  repos=$((repos + 1))

  # 1. Retire stale shims: any of ours whose name is not in the current set.
  #    Since checks moved in-process this removes the 16 sub-hook shims too —
  #    they are no longer installed, and the binary no longer answers to their
  #    names, so leaving one behind means a commit dies on `unknown hook`.
  for f in "$hooks"/*; do
    [ -f "$f" ] || continue
    name=$(basename "$f")
    case "$name" in *.sample) continue ;; esac
    [ -e "$SRC/$name" ] && continue          # still shipped — leave it
    is_ours "$f" || continue                 # not ours — leave it
    say "rm  $f"
    [ "$APPLY" = "1" ] && rm -f "$f"
    removed=$((removed + 1))
  done

  # 2. Hand-written sub-hooks. `pre-push-branch-protect.sh` in two repos is now
  #    a built-in check; anything else here would be dispatched by a glob that
  #    no longer exists, so it would silently stop running. Removing it is the
  #    honest outcome — a file that never runs is worse than no file.
  #    Step 1 already retired OUR files; this is only for the ones it skips
  #    because is_ours() says they are somebody else's. Without the exclusion
  #    both steps claim the same files and the counts double.
  for f in "$hooks"/pre-commit-* "$hooks"/pre-push-*; do
    [ -f "$f" ] || continue
    is_ours "$f" && continue
    say "rm  $f  (sub-hooks are in-process now)"
    [ "$APPLY" = "1" ] && rm -f "$f"
    foreign=$((foreign + 1))
  done

  # 3. The vestigial node-era package.json: it forced CommonJS for the .js
  #    hooks, and no hook is node any more. Match on content, not just name,
  #    so a repo that put its own package.json here keeps it.
  if [ -f "$hooks/package.json" ] && grep -q 'Forces Node' "$hooks/package.json" 2>/dev/null; then
    say "rm  $hooks/package.json"
    [ "$APPLY" = "1" ] && rm -f "$hooks/package.json"
    pkgjson=$((pkgjson + 1))
  fi

  # 4. Install the current set, baking the absolute binary path. Git hooks do
  #    not inherit an interactive PATH, so a PATH-only shim fails under GUI
  #    clients (see the Makefile's bake-shims).
  for s in "$SRC"/*; do
    [ -f "$s" ] || continue
    dst="$hooks/$(basename "$s")"
    if [ "$APPLY" = "1" ]; then
      sed "s|__GITHOOKS_BIN__|$BIN|g" "$s" > "$dst"
      chmod +x "$dst"
    fi
    installed=$((installed + 1))
  done
done

echo
echo "repos (managed):  $repos"
echo "stale shims:      $removed"
echo "foreign sub-hooks: $foreign"
echo "package.json:     $pkgjson"
echo "shims installed:  $installed"
echo "skipped (unmanaged, e.g. app data repos): $skipped"
[ "$APPLY" = "1" ] || echo "\n(dry run — re-run with --apply)"
