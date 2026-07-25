#!/bin/zsh
# Lint staged files
# Author: https://github.com/fredericrous
ERROR_SIGN=$'  \e[38;5;160m✗\e[0m'
VALID_SIGN=$'  \e[38;5;112m✓\e[0m'
WARNING_SIGN=$'  \e[38;5;208m!\e[0m'

FILES=`git diff --diff-filter=d --cached --name-only | grep -E '\.(js|jsx|ts|tsx|vue)$'`
[ ${#FILES} -lt 1 ] && exit

# Skip when the repo has no ESLint config — ESLint 9+ errors out ("couldn't
# find an eslint.config file") instead of no-op'ing, which would fail commits in
# repos that don't lint JS (e.g. this templates repo, infra/manifest repos).
# (N) = null-glob: expands to nothing (no zsh nomatch error) when absent.
ROOT=`git rev-parse --show-toplevel`
configs=("$ROOT"/eslint.config.{js,mjs,cjs,ts}(N) "$ROOT"/.eslintrc*(N))
if [ ${#configs} -eq 0 ] && ! grep -q '"eslintConfig"' "$ROOT/package.json" 2>/dev/null; then
    printf "$VALID_SIGN ESLint skipped (no eslint config)\n"
    exit 0
fi

# Resolve eslint: prefer the repo's PINNED eslint (node_modules/.bin) so the
# hook matches CI. A linked worktree has no node_modules; fall back to the
# MAIN worktree's (shared git-common-dir's parent). Only then PATH eslint,
# and `npx --no-install` (never silently download a random latest).
common_dir=`git rev-parse --path-format=absolute --git-common-dir 2>/dev/null`
if [[ -x "$ROOT/node_modules/.bin/eslint" ]]; then
    ESLINT=("$ROOT/node_modules/.bin/eslint")
elif [[ -n "$common_dir" && -x "${common_dir:h}/node_modules/.bin/eslint" ]]; then
    ESLINT=("${common_dir:h}/node_modules/.bin/eslint")
elif type eslint > /dev/null; then
    ESLINT=(eslint)
elif type npx > /dev/null 2>&1 && npx --no-install eslint --version > /dev/null 2>&1; then
    # --no-install: never silently download a random latest eslint.
    ESLINT=(npx --no-install eslint)
else
    # Repo opts into eslint (config present) but no eslint binary is resolvable.
    # Warn + skip rather than block the commit with a false "issues found" — a
    # non-zero exit from a MISSING eslint is not the same as lint errors.
    printf "$WARNING_SIGN eslint config found but no eslint binary. Run \033[38;5;208mnpm install\033[0m\n"
    exit 0
fi
$ESLINT `printf ${FILES[*]}` "$@"

if [ $? -ne 0 ]; then
    printf "$ERROR_SIGN ESLint issues found. Please fix\n"
    exit 1
fi
printf "$VALID_SIGN ESLint passed\n"
