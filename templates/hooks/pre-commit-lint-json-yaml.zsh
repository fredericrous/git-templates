#!/bin/zsh
# Lint staged files
# Author: https://github.com/fredericrous
ERROR_SIGN="  \u001b[38;5;160m\u2717\u001b[0m"
VALID_SIGN="  \u001b[38;5;112m\u2713\u001b[0m"
WARNING_SIGN="  \u001b[38;5;208m!\u001b[0m"

FILES=`git diff --diff-filter=d --cached --name-only | grep -E '\.(json|yaml)$'`
[ ${#FILES} -lt 1 ] && exit 0

if ! type yq > /dev/null; then
    printf "$WARNING_SIGN Json/Yaml files detected. To lint them, install \u001b[38;5;208myq\u001b[0m"
    exit 0
fi

yq e 'true' `printf ${FILES[*]}` 1>/dev/null
if [ $? -ne 0 ]; then
    printf "$ERROR_SIGN Json/Yaml Lint issues found. Please fix\n"
    exit 1
fi
printf "$VALID_SIGN Json/Yaml Lint passed\n"
