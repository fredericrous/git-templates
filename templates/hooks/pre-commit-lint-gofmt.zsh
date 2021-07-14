#!/bin/zsh
# Lint staged files
# Author: https://github.com/fredericrous
ERROR_SIGN="  \u001b[38;5;160m\u2717\u001b[0m"
VALID_SIGN="  \u001b[38;5;112m\u2713\u001b[0m"

FILES=`git diff --diff-filter=d --cached --name-only | grep -E '\.(go)$'`
[ ${#FILES} -lt 1 ] && exit

go fmt ./... 2>&1 | read

if [ $? -ne 0 ]; then
    printf "$ERROR_SIGN Gofmt issues found. Please fix\n"
    exit 1
fi
printf "$VALID_SIGN Gofmt passed\n"
