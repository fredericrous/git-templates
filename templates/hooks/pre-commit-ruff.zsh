#!/bin/zsh
# Ruff lint + format check on staged Python files, scoped to repos that
# opt into ruff. Catches BOTH classes a CI ruff step enforces:
#   - `ruff check`        -- lint rules
#   - `ruff format --check` -- formatting
# CI runs these as TWO SEPARATE steps, so a clean `ruff check` does NOT
# imply a clean `ruff format`. Skipping the format check locally is how a
# format-only failure slips through to CI (burned 2026-06-02).
#
# Scoping: only runs when the repo opts into ruff — a `[tool.ruff]` table
# in pyproject.toml at the git root, or a ruff.toml / .ruff.toml. Without
# that signal we skip, so this never fires in repos that don't use ruff.
#
# Binary resolution: prefer a project-local .venv/bin/ruff (pinned
# version), then `ruff` on PATH, then `uvx ruff` (matches uv-based CI).
# Falls back to warn+skip when none resolve. `--force-exclude` makes ruff
# honour the repo's own exclude config even though we pass explicit paths.
# Author: https://github.com/fredericrous
ERROR_SIGN=$'  \e[38;5;160m✗\e[0m'
VALID_SIGN=$'  \e[38;5;112m✓\e[0m'
WARNING_SIGN=$'  \e[38;5;208m!\e[0m'

staged=(${(f)"$(git diff --diff-filter=d --cached --name-only)"})
(( ${#staged} == 0 )) && exit 0
files=(${(f)"$(printf '%s\n' $staged | grep -E '\.pyi?$')"})
(( ${#files} == 0 )) && exit 0

ROOT=$(git rev-parse --show-toplevel 2>/dev/null) || exit 0

# --- decide whether this repo opts into ruff -------------------------
# (N) null-glob: unmatched patterns vanish instead of erroring (zsh NOMATCH).
has_config=0
config_matches=("$ROOT"/ruff.toml(N.) "$ROOT"/.ruff.toml(N.))
(( ${#config_matches} > 0 )) && has_config=1
if (( ! has_config )) && [[ -f "$ROOT/pyproject.toml" ]]; then
    grep -q '^\[tool\.ruff' "$ROOT/pyproject.toml" && has_config=1
fi
(( ! has_config )) && exit 0

# --- resolve a ruff binary -------------------------------------------
if [[ -x "$ROOT/.venv/bin/ruff" ]]; then
    RUFF=("$ROOT/.venv/bin/ruff")
elif type ruff > /dev/null 2>&1; then
    RUFF=(ruff)
elif type uvx > /dev/null 2>&1; then
    RUFF=(uvx ruff)
else
    printf "$WARNING_SIGN ruff config found but no ruff/uvx binary. Install ruff or uv.\n"
    exit 0
fi

fail=0

if ! "${RUFF[@]}" check --force-exclude $files > /dev/null 2>&1; then
    printf "$ERROR_SIGN Ruff lint issues. Run \033[38;5;208mruff check --fix\033[0m. Offenders:\n"
    "${RUFF[@]}" check --force-exclude $files 2>&1 | sed 's/^/      /'
    fail=1
fi

if ! "${RUFF[@]}" format --check --force-exclude $files > /dev/null 2>&1; then
    printf "$ERROR_SIGN Ruff found unformatted files. Run \033[38;5;208mruff format\033[0m on:\n"
    "${RUFF[@]}" format --check --force-exclude $files 2>&1 \
        | grep -iE 'would reformat' | sed 's/^/      /'
    fail=1
fi

(( fail )) && exit 1
printf "$VALID_SIGN Ruff passed\n"
