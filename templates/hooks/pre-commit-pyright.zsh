#!/bin/zsh
# Pyright type-check on staged Python files, scoped to repos that opt into
# pyright. CI commonly runs `pyright` (often strict) over the WHOLE tree,
# so a per-file error in a file you didn't manually check — a test fixture,
# a helper — slips through to CI. This runs pyright over every staged .py
# so the same class of error fails locally first (burned 2026-07-07: a
# strict-mode arg-type error in a staged test file passed a scoped local
# check but failed CI's whole-tree pyright).
#
# Scoping: only runs when the repo opts into pyright — a `[tool.pyright]`
# table in pyproject.toml at the git root, or a pyrightconfig.json /
# pyrightconfig.jsonc. Without that signal we skip, so this never fires in
# repos that don't use pyright.
#
# Scope vs speed: checks the STAGED files, not the whole tree — fast enough
# for a pre-commit hook while catching the per-file errors that are the
# usual local-vs-CI gap. Pyright still resolves the whole workspace for
# imports, so type inference is unchanged; only the reported set is scoped.
# CI's whole-tree run stays the authority for cross-file-only errors.
#
# Binary resolution: prefer a project-local .venv/bin/pyright (pinned
# version), then `pyright` on PATH, then `uvx pyright` (matches uv-based
# CI). Falls back to warn+skip when none resolve. Pyright cold-starts a
# bundled node on first run (cached after), so the first commit in a fresh
# environment is slower.
# Author: https://github.com/fredericrous
ERROR_SIGN=$'  \e[38;5;160m✗\e[0m'
VALID_SIGN=$'  \e[38;5;112m✓\e[0m'
WARNING_SIGN=$'  \e[38;5;208m!\e[0m'

staged=(${(f)"$(git diff --diff-filter=d --cached --name-only)"})
(( ${#staged} == 0 )) && exit 0
files=(${(f)"$(printf '%s\n' $staged | grep -E '\.pyi?$')"})
(( ${#files} == 0 )) && exit 0

ROOT=$(git rev-parse --show-toplevel 2>/dev/null) || exit 0

# --- decide whether this repo opts into pyright ----------------------
# (N) null-glob: unmatched patterns vanish instead of erroring (zsh NOMATCH).
has_config=0
config_matches=("$ROOT"/pyrightconfig.json(N.) "$ROOT"/pyrightconfig.jsonc(N.))
(( ${#config_matches} > 0 )) && has_config=1
if (( ! has_config )) && [[ -f "$ROOT/pyproject.toml" ]]; then
    grep -q '^\[tool\.pyright' "$ROOT/pyproject.toml" && has_config=1
fi
(( ! has_config )) && exit 0

# --- resolve a pyright binary ----------------------------------------
if [[ -x "$ROOT/.venv/bin/pyright" ]]; then
    PYRIGHT=("$ROOT/.venv/bin/pyright")
elif type pyright > /dev/null 2>&1; then
    PYRIGHT=(pyright)
elif type uvx > /dev/null 2>&1; then
    PYRIGHT=(uvx pyright)
else
    printf "$WARNING_SIGN pyright config found but no pyright/uvx binary. Install pyright or uv.\n"
    exit 0
fi

# Pass repo-relative paths so pyright's include/exclude config applies.
typeset -a rel_files
for f in $files; do
    rel_files+=("${f#$ROOT/}")
done

if ! "${PYRIGHT[@]}" $rel_files > /dev/null 2>&1; then
    printf "$ERROR_SIGN Pyright type errors in staged files:\n"
    "${PYRIGHT[@]}" $rel_files 2>&1 | grep -iE 'error:|warning:' | sed 's/^/      /'
    exit 1
fi
printf "$VALID_SIGN Pyright passed\n"
