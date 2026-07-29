#!/bin/zsh
# Sync branch with ITS OWN upstream before push; warn (don't act) if the default
# branch is ahead.
# Author: https://github.com/fredericrous
#
# Hardened 2026-06: the old version ran `git pull --rebase origin HEAD`, where the
# remote ref `HEAD` resolves to the remote's DEFAULT branch (main) — so every push
# silently rebased the current branch onto main, autostashing uncommitted work and
# conflicting with squash-merged history. Now it: (1) never touches a dirty tree,
# (2) rebases onto the branch's own upstream (never main), (3) aborts cleanly on
# conflict instead of leaving a half-rebased state.
ERROR_SIGN=$'  \e[38;5;160m✗\e[0m'
WARNING_SIGN=$'  \e[38;5;208m!\e[0m'
VALID_SIGN=$'  \e[38;5;112m✓\e[0m'

# 1. Never auto-rebase with uncommitted changes — that autostashes your work and
#    can leave a broken mid-rebase state during a push.
if [[ -n "$(git status --porcelain)" ]]; then
    printf "$WARNING_SIGN Uncommitted changes — skipping pre-push pull-rebase.\n"
    exit 0
fi

# 2. Only sync a branch that already has an upstream, and rebase onto THAT
#    upstream (origin/<this-branch>) — never the default branch. A brand-new
#    branch (no upstream yet) has nothing to sync.
if ! git rev-parse --abbrev-ref --symbolic-full-name @{u} > /dev/null 2>&1; then
    exit 0
fi

# 2b. The upstream may be configured locally but GONE on the remote — the normal
#     state right after a PR squash-merges with delete-branch-on-merge (e.g. when
#     pushing a release tag from the merged branch). `git pull --rebase` would
#     fail on the missing ref and read as a conflict, wrongly blocking the push.
#     Nothing to sync with — skip. (ls-remote costs a round-trip, but so does the
#     push we're about to do.)
UPSTREAM=$(git rev-parse --abbrev-ref --symbolic-full-name @{u} 2>/dev/null)
UPSTREAM_REMOTE=${UPSTREAM%%/*}
UPSTREAM_BRANCH=${UPSTREAM#*/}
if ! git ls-remote --exit-code --heads "$UPSTREAM_REMOTE" "$UPSTREAM_BRANCH" > /dev/null 2>&1; then
    printf "$WARNING_SIGN Upstream $UPSTREAM no longer exists on the remote (merged + auto-deleted?) — skipping sync.\n"
    exit 0
fi

# rg is the fast path but isn't everywhere, and an unguarded rg that's missing
# would leave HAS_DIVERGED empty and silently auto-rebase a diverged branch —
# the one case this guard exists to prevent. ONE pattern per check: POSIX
# classes are understood by rg and grep -E alike (\s and \d are not), so the
# fallback can't drift. HOOKS_FORCE_GREP=1 exercises it where rg IS installed.
DIVERGED_REGEX='ahead[[:space:]]+[0-9]+,[[:space:]]*behind'
if [[ -z $HOOKS_FORCE_GREP ]] && command -v rg > /dev/null 2>&1; then
    diverged_count() { rg -c "$DIVERGED_REGEX" }
    default_branch_is() { rg -q "^[[:space:]*]+$1\$" }
else
    diverged_count() { grep -cE "$DIVERGED_REGEX" }
    default_branch_is() { grep -qE "^[[:space:]*]+$1\$" }
fi

# `grep -c` prints 0 on no match where `rg -c` prints nothing; both leave the
# test below false, so the two paths agree.
HAS_DIVERGED=`git status -sb | diverged_count`
if [[ $HAS_DIVERGED -eq 1 ]]; then
    printf "$WARNING_SIGN Branch diverged from its upstream — skip auto pull-rebase.\n"
    printf "    Reconcile manually: \033[38;5;208mgit pull --rebase\033[0m (or \033[38;5;208mgit merge\033[0m)\n"
elif ! git pull --rebase > /dev/null 2>&1; then
    git rebase --abort > /dev/null 2>&1
    printf "$ERROR_SIGN pull --rebase hit conflicts (rebase aborted, tree restored).\n"
    printf "    Resolve manually: \033[38;5;208mgit pull --rebase\033[0m\n"
    exit 1
else
    printf "$VALID_SIGN Branch is in sync with its upstream\n"
fi

# 3. Informational only: warn if the default branch has moved ahead. No auto-action.
BRANCH_LIST=`git branch`
if printf '%s\n' "$BRANCH_LIST" | default_branch_is main; then
    DEFAULT_BRANCH="main"
elif printf '%s\n' "$BRANCH_LIST" | default_branch_is master; then
    DEFAULT_BRANCH="master"
else
    exit 0
fi
git fetch origin $DEFAULT_BRANCH > /dev/null 2>&1
AHEAD_COMMITS=`git rev-list --left-right --count origin/$DEFAULT_BRANCH...HEAD 2>/dev/null | head -c 1`
if [[ -n "$AHEAD_COMMITS" && ! $AHEAD_COMMITS = 0 ]]; then
    printf "$WARNING_SIGN origin/$DEFAULT_BRANCH is ahead by $AHEAD_COMMITS commit(s).\n"
    printf "    Consider before merging: \033[38;5;208mgit merge origin/$DEFAULT_BRANCH\033[0m\n"
fi
