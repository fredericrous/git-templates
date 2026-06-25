#!/bin/zsh
# Lint staged files
# Author: https://github.com/fredericrous
ERROR_SIGN=$'  \e[38;5;160m✗\e[0m'
VALID_SIGN=$'  \e[38;5;112m✓\e[0m'

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

if ! type eslint > /dev/null; then
    npx eslint `printf ${FILES[*]}` "$@"
else
    eslint `printf ${FILES[*]}` "$@"
fi

if [ $? -ne 0 ]; then
    printf "$ERROR_SIGN ESLint issues found. Please fix\n"
    exit 1
fi
printf "$VALID_SIGN ESLint passed\n"
