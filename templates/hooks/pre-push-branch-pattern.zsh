#!/bin/zsh
# Check a branch name is well formated before push,
# unless it's already there on the remote
# Author: https://github.com/fredericrous
ERROR_SIGN="  \u001b[38;5;160m\u2717\u001b[0m"
VALID_SIGN="  \u001b[38;5;112m\u2713\u001b[0m"

BRANCH_REGEX='^(feat|fix|hotfix|chore|test|automation)/[\w-]+$'

LOCAL_BRANCH=`git rev-parse --abbrev-ref HEAD`
[[ $? -ne 0 ]] && exit 0
if git show-branch remotes/origin/$LOCAL_BRANCH &> /dev/null; then
    printf "$VALID_SIGN Branch already on server. Name is authorized.\n"
    exit 0
fi
if ! echo $LOCAL_BRANCH | rg $BRANCH_REGEX -oc &> /dev/null; then
    printf "$ERROR_SIGN Branch names in this project must adhere to this contract:
    \u001b[38;5;208m${BRANCH_REGEX}\u001b[0m.
    Rename your branch with: \u001b[38;5;208mgit branch -m\u001b[0m <branch name>\n"
    exit 1
fi
printf "$VALID_SIGN Branch name conforms with authorized pattern\n"

