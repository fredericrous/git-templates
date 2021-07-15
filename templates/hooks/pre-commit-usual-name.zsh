#!/bin/zsh
# Issue a warning if it's the first time the author commits with this name
# Author: https://github.com/fredericrous
WARNING_SIGN="  \u001b[38;5;208m!\u001b[0m"

USER_EMAIL=`git config user.email`
USER_NAME=`git config user.name`
FULL_NAME="$USER_NAME <$USER_EMAIL>"

git log -1 > /dev/null || exit 0

COMMITS_PER_AUTHOR=`git shortlog -s -n -e --all`
if ! echo $COMMITS_PER_AUTHOR | rg --context=0 "$FULL_NAME" &> /dev/null; then
    printf "$WARNING_SIGN It is the first time you commit as \u001b[38;5;208m$FULL_NAME\u001b[0m\n"
fi
