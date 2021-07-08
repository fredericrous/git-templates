#!/bin/zsh
# Check a branch name is well formated before push,
# unless it's already there on the remote
# Author: https://github.com/fredericrous
ERROR_SIGN="  \u001b[38;5;160m\u2717\u001b[0m"

BRANCH_REGEX='^(feat|fix|hotfix|chore|test|automation)/\d+-[\w-]+$'

LOCAL_BRANCH=`git rev-parse --abbrev-ref HEAD`

if  ! git show-branch remotes/origin/$LOCAL_BRANCH &> /dev/null && ! echo $LOCAL_BRANCH | rg $BRANCH_REGEX -oc &> /dev/null; then
    printf "$ERROR_SIGN Branch names in this project must adhere to this contract:\n    \u001b[38;5;208m${BRANCH_REGEX}\u001b[0m.
    Rename your branch with: \u001b[38;5;208mgit branch -m\u001b[0m <branch name>\n"
    exit 1
fi
