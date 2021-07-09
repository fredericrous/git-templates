#!/bin/zsh
# Look for files in merge state
# Author: https://github.com/fredericrous
ERROR_SIGN="  \u001b[38;5;160m\u2717\u001b[0m"
VALID_SIGN="  \u001b[38;5;112m\u2713\u001b[0m"

FILES_IN_CONFLICT=(`git grep --cached  -e '>>>>>' --or -e '=======' --or -e '>>>>>>>' --all-match --files-with-matches`)

if [[ ${#FILES_IN_CONFLICT[*]} -ne 0 ]]; then
    printf "$ERROR_SIGN Merge conflict detected in \u001b[38;5;208m${FILES_IN_CONFLICT[*]}\u001b[0m\n"
    exit 1
fi

printf "$VALID_SIGN No merge confict detected\n"
