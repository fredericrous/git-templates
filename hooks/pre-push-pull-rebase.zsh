#!/bin/zsh
# Sync branch before push
# Issue a warning if remote default branch is ahead
# Author: https://github.com/fredericrous
ERROR_SIGN="  \u001b[38;5;160m\u2717\u001b[0m"
WARNING_SIGN="  \u001b[38;5;208m!\u001b[0m"
VALID_SIGN="  \u001b[38;5;112m\u2713\u001b[0m"

DEFAULT_BRANCH=`git rev-parse --abbrev-ref origin/HEAD | sed 's@origin/@@'`
# default branch = HEAD when there is no head yet on the server
[[ $DEFAULT_BRANCH = "HEAD" ]] && exit 0

if ! git pull --rebase origin HEAD; then
    printf "$ERROR_SIGN Couldn't complete pull rebase. You might want to do a git merge instead"
    printf "    \u001b[38;5;208mgit fetch\u001b[0m && \u001b[38;5;208mgit merge origin $DEFAULT_BRANCH\u001b[0m"
    exit 1
fi

git fetch origin $DEFAULT_BRANCH
AHEAD_COMMITS=`git rev-list --left-right --count origin/$DEFAULT_BRANCH...HEAD | head -c 1`
if [[ ! $AHEAD_COMMITS = 0 ]]; then
    printf "$WARNING_SIGN $DEFAULT_BRANCH is ahead of current branch by $AHEAD_COMMITS commits.\n    You might want to execute: git merge origin/$DEFAULT_BRANCH && git push origin HEAD\n"
fi
