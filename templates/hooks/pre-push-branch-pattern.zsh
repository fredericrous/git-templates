#!/bin/zsh
# Check a branch name is well formated before push,
# unless it's already there on the remote
# Author: https://github.com/fredericrous
ERROR_SIGN=$'  \e[38;5;160m✗\e[0m'
VALID_SIGN=$'  \e[38;5;112m✓\e[0m'

# Dots are allowed only under chore/ — version-bump branches read naturally
# (chore/duro-1.50.50); feature/fix names stay dot-free. Git itself already
# rejects the dangerous dot forms ("..", trailing ".lock").
#
# ONE pattern, both engines: POSIX classes ([[:alnum:]]) are understood by
# rg (Rust regex) and grep -E alike, where \w is not — so the fallback can't
# drift from the fast path, and the contract printed on failure below is the
# same text whichever binary the machine has.
BRANCH_REGEX='^((feat|fix|hotfix|test|automation)/[[:alnum:]_-]+|chore/[[:alnum:]_.-]+)$'

# rg is the fast path but is NOT everywhere, and an unguarded `! rg …` is true
# when the binary is missing — which made this hook reject EVERY branch name on
# a machine without it. HOOKS_FORCE_GREP=1 exercises the fallback where rg IS
# installed, so the tests cover both.
if [[ -z $HOOKS_FORCE_GREP ]] && command -v rg > /dev/null 2>&1; then
    branch_matches() { rg -q "$BRANCH_REGEX" }
else
    branch_matches() { grep -qE "$BRANCH_REGEX" }
fi

LOCAL_BRANCH=`git rev-parse --abbrev-ref HEAD`
[[ $? -ne 0 ]] && exit 0
if git show-branch remotes/origin/$LOCAL_BRANCH &> /dev/null; then
    printf "$VALID_SIGN Branch already on server. Name is authorized.\n"
    exit 0
fi
# Initial push to a brand-new empty remote (no branches yet): there's no
# feature-branch convention to enforce when you're initializing the repo, and the
# default branch (main/master) doesn't match the pattern. Allow it. ($1 is the
# remote name passed by git to pre-push.)
REMOTE_NAME="${1:-origin}"
if [[ -z "$(git ls-remote --heads "$REMOTE_NAME" 2>/dev/null)" ]]; then
    printf "$VALID_SIGN Remote has no branches yet (initial push). Name is authorized.\n"
    exit 0
fi
if ! printf '%s\n' "$LOCAL_BRANCH" | branch_matches; then
    printf "$ERROR_SIGN Branch names in this project must adhere to this contract:
    \033[38;5;208m${BRANCH_REGEX}\033[0m.
    Rename your branch with: \033[38;5;208mgit branch -m\033[0m <branch name>
    Or bypass this check with git -c hook.skip=branch-pattern push\n"
    exit 1
fi
printf "$VALID_SIGN Branch name conforms with authorized pattern\n"

