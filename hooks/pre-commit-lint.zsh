#!/bin/zsh
# Lint staged files
# Author: https://github.com/fredericrous
ERROR_SIGN="  \u001b[38;5;160m\u2717\u001b[0m"
VALID_SIGN="  \u001b[38;5;112m\u2713\u001b[0m"

FILES=`git diff --diff-filter=d --cached --name-only | grep -E '\.(js|jsx|vue)$'`
[ ${#FILES} -lt 1 ] && exit

if ! type eslint > /dev/null; then
    npx eslint ${FILES[*]} "$@"
else
    eslint ${FILES[*]} "$@"
fi

if [ $? -ne 0 ]; then
    printf "$ERROR_SIGN Lint issues found. Please fix\n"
    exit 1
fi
printf "$VALID_SIGN Lint passed\n"
