#!/bin/zsh
# Issue a warning if it's the first time the author commits with this name
# Author: https://github.com/fredericrous
WARNING_SIGN=$'  \e[38;5;208m!\e[0m'

USER_EMAIL=`git config user.email`
USER_NAME=`git config user.name`
FULL_NAME="$USER_NAME <$USER_EMAIL>"

git log -1 > /dev/null || exit 0

# rg is the fast path but isn't everywhere, and an unguarded `! rg …` is true
# when it's missing — here that means warning "first time you commit as …" on
# every commit. -F/fixed-string either way: a real name can contain regex
# metacharacters (O'Brien, "Foo (Bar)"), which as a pattern would misfire.
# HOOKS_FORCE_GREP=1 exercises the fallback where rg IS installed.
if [[ -z $HOOKS_FORCE_GREP ]] && command -v rg > /dev/null 2>&1; then
    name_seen() { rg -qF "$FULL_NAME" }
else
    name_seen() { grep -qF -- "$FULL_NAME" }
fi

COMMITS_PER_AUTHOR=`git shortlog -s -n -e --all`
if ! printf '%s\n' "$COMMITS_PER_AUTHOR" | name_seen; then
    printf "$WARNING_SIGN It is the first time you commit as \033[38;5;208m$FULL_NAME\033[0m\n"
fi
