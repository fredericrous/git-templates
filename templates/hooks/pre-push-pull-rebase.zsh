#!/bin/zsh
# Sync branch before push
# Issue a warning if remote default branch is ahead
# Author: https://github.com/fredericrous
ERROR_SIGN="  \u001b[38;5;160m\u2717\u001b[0m"
WARNING_SIGN="  \u001b[38;5;208m!\u001b[0m"
VALID_SIGN="  \u001b[38;5;112m\u2713\u001b[0m"

BRANCH_LIST=`git branch`
if echo $BRANCH_LIST | rg '^[\s*]+main$' --context=0 -or '$1'; then
    DEFAULT_BRANCH="main"
elif echo $BRANCH_LIST | rg '^[\s*]+master$' --context=0 -or '$1'; then
    DEFAULT_BRANCH="master"
else
    DEFAULT_BRANCH=`git ls-remote --heads -q | rg 'refs/heads/(.*)' --context=0 -or '$1'`;
    [[ $? -ne 0 ]] && exit 0
fi

HAS_DIVERGED=`git status -sb | rg 'ahead\s\d+,\sbehind' -c`
if [[ $HAS_DIVERGED -eq 1 ]]; then
    printf "$WARNING_SIGN branches diverged. Skip pull rebase\n"
    SUGGESTED_COMMAND="git reset --soft HEAD~1 && git pull"
    printf "    You might want to execute: \u001b[38;5;208m$SUGGESTED_COMMAND\u001b[0m\n"
elif ! git pull --rebase origin HEAD; then
    printf "$ERROR_SIGN Couldn't complete pull rebase. You might want to do a git merge instead"
    printf "    \u001b[38;5;208mgit fetch\u001b[0m && \u001b[38;5;208mgit merge origin $DEFAULT_BRANCH\u001b[0m"
    exit 1
fi

git fetch origin $DEFAULT_BRANCH
AHEAD_COMMITS=`git rev-list --left-right --count origin/$DEFAULT_BRANCH...HEAD | head -c 1`
if [[ ! $AHEAD_COMMITS = 0 ]]; then
    printf "$WARNING_SIGN origin/$DEFAULT_BRANCH is ahead of current branch by $AHEAD_COMMITS commits.\n"
    if [[ $HAS_DIVERGED -ne 1 ]]; then
        SUGGESTED_COMMAND="git merge origin/$DEFAULT_BRANCH && git push origin HEAD"
        printf "    You might want to execute: \u001b[38;5;208m$SUGGESTED_COMMAND\u001b[0m\n"
    fi
fi
