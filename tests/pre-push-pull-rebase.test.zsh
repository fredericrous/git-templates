#!/bin/zsh
# Runs inside the runner's disposable repo (already cd'd in). Validates the
# hardened guards: it must never block a push or touch a dirty tree, and rebases
# onto the branch's own upstream (never the default branch).
TEST_NAME=`basename "$0"`
HOOK=`echo ${0:a:h}/../templates/hooks/$TEST_NAME | sed 's@\.test@@'`

git config user.email test@example.com
git config user.name test
# Born the HEAD (the runner's repo is unborn). --no-verify: don't trip the hooks.
git commit -q --allow-empty --no-verify -m init

printf "Should pass (skip) on a new branch with no upstream\n"
$HOOK &> /dev/null || exit 1

printf "Should skip and NOT autostash a dirty tree\n"
echo dirty > scratch.txt                       # untracked change => dirty
$HOOK &> /dev/null || exit 1
[[ -f scratch.txt ]] || exit 1                  # still there (not stashed away)
git status --porcelain | grep -q 'scratch.txt' || exit 1   # still uncommitted
rm -f scratch.txt
# NB: at this point there is no upstream, so the hook would exit early anyway —
# this case alone cannot tell the dirty-tree guard from the no-upstream one.
# The real assertion is repeated below, once an upstream EXISTS.

printf "Should pass when in sync with its own upstream (no rebase onto main)\n"
git init -q --bare ./origin.git
git remote add origin "$PWD/origin.git"
# The bare origin lives INSIDE the working tree: ignore it (and commit the
# .gitignore) or every later assertion silently takes the dirty-tree early-exit
# instead of the code path it claims to test. Append (>>) — never truncate a
# .gitignore an earlier step may have written.
echo 'origin.git/' >> .gitignore
git add .gitignore && git commit -q --no-verify -m 'ignore local origin'
git push -q --no-verify -u origin HEAD          # sets upstream = origin/<this branch>
$HOOK &> /dev/null || exit 1
$HOOK 2>&1 | grep -q "in sync" || exit 1        # really reached the sync path (not the dirty skip)

printf "Should pass (skip) when the upstream branch was deleted on the remote\n"
# The post-squash-merge state under delete-branch-on-merge: upstream still
# configured locally, branch gone remotely (e.g. pushing a release tag next).
# Delete on the REMOTE side (like GitHub's delete-branch-on-merge) so the local
# remote-tracking ref survives and @{u} still resolves — `push --delete` from
# this clone would drop the tracking ref too and dodge the code path under test.
git checkout -q -b feat/merged-away
git push -q --no-verify -u origin feat/merged-away
git -C ./origin.git update-ref -d refs/heads/feat/merged-away
$HOOK &> /dev/null || exit 1
# And it must be the explicit SKIP, not a blocked push:
$HOOK 2>&1 | grep -q "no longer exists" || exit 1

# The dirty-tree guard, for real this time. The earlier case asserted only that
# the file survived, which the no-upstream exit satisfies just as well — delete
# the guard entirely and that case still passed. The guard is step 1, so it
# fires whatever the upstream state; assert its OWN message and nothing else can
# produce a pass.
printf "Should announce the dirty-tree skip, not just leave the file alone\n"
echo dirty > scratch2.txt
$HOOK 2>&1 | grep -q "Uncommitted changes" || { echo "  guard did not fire"; exit 1 }
[[ -f scratch2.txt ]] || exit 1
rm -f scratch2.txt

# The grep fallback carries the divergence guard on machines without rg. If it
# silently matched nothing, HAS_DIVERGED would read as "not diverged" and the
# hook would auto-rebase the very case it exists to refuse.
printf "Should reach the same verdict when falling back to grep\n"
HOOKS_FORCE_GREP=1 $HOOK &> /dev/null || exit 1
HOOKS_FORCE_GREP=1 $HOOK 2>&1 | grep -q "no longer exists" || exit 1

exit 0
