#!/bin/zsh
# Look for files in merge state
# Author: https://github.com/fredericrous
ERROR_SIGN=$'  \e[38;5;160m✗\e[0m'
VALID_SIGN=$'  \e[38;5;112m✓\e[0m'
SCRIPT_NAME=`basename "$0"`
TEST_HOOK_PATH=tests/${SCRIPT_NAME:s/.zsh/.test.zsh}
HOOK_PATH=templates/hooks/$SCRIPT_NAME

FILES_IN_CONFLICT=(`git grep --cached  -e '<<<<<<<' --or -e '=======' --or -e '>>>>>>>' --all-match --files-with-matches`)
FILES_IN_CONFLICT=(${FILES_IN_CONFLICT[@]:#$TEST_HOOK_PATH})
FILES_IN_CONFLICT=(${FILES_IN_CONFLICT[@]:#$HOOK_PATH})
if [[ ${#FILES_IN_CONFLICT[*]} -ne 0 ]]; then
    printf "$ERROR_SIGN Merge conflict detected in \033[38;5;208m${FILES_IN_CONFLICT[@]:s: :, :}\033[0m\n"
    exit 1
fi

printf "$VALID_SIGN No merge confict detected\n"
