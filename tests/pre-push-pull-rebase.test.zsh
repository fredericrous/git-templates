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

printf "Should pass when in sync with its own upstream (no rebase onto main)\n"
git init -q --bare ./origin.git
git remote add origin "$PWD/origin.git"
git push -q --no-verify -u origin HEAD          # sets upstream = origin/<this branch>
$HOOK &> /dev/null || exit 1

exit 0
