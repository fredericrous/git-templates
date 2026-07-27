#!/bin/zsh
TEST_NAME=`basename "$0"`
HOOK_CHECK=`echo ${0:a:h}/../templates/hooks/$TEST_NAME | sed 's@\.test@@'`

# The runner's throwaway repo is unborn, and the hook bails out as "pass"
# whenever `git rev-parse --abbrev-ref HEAD` fails (unborn HEAD) — so without a
# commit no branch name is ever evaluated and the "should throw" case can't
# throw. Born the HEAD with an empty commit (--no-verify: don't trip the very
# hooks under test). Everything below stays inside this disposable repo; we
# never rename the parent's branch.
git -c user.email=test@example.com -c user.name=test commit -q --allow-empty --no-verify -m init

# The hook authorizes any name when the remote has no branches yet (initial
# push) — and `git ls-remote --heads origin` is also empty when there's no
# origin at all, as in this throwaway repo. That guard was short-circuiting
# every case below into a pass. Give the repo a real (local bare) origin with
# one branch so the guard stays out of the way and the regex is exercised.
git init -q --bare .fake-origin.git
git remote add origin .fake-origin.git
git push -q --no-verify origin HEAD:main &> /dev/null

printf "Should pass when HEAD cannot be resolved\n"
saved_head=$(git symbolic-ref HEAD)
echo "ref: refs/heads/__nonexistent__" > .git/HEAD
$HOOK_CHECK &> /dev/null || exit 1
git symbolic-ref HEAD "$saved_head"

printf "Should pass when branch is already on the server\n"
git branch -m off-pattern                              # a name that fails the regex...
git update-ref refs/remotes/origin/off-pattern HEAD    # ...but exists on origin
$HOOK_CHECK &> /dev/null || exit 1
git update-ref -d refs/remotes/origin/off-pattern

printf "Should throw when branch does not conform\n"
$HOOK_CHECK &> /dev/null && exit 1                      # still on off-pattern, no origin ref

printf "Should pass when branch conforms\n"
git branch -m feat/0-test
$HOOK_CHECK &> /dev/null || exit 1

printf "Should pass when a chore branch carries a semver (dots)\n"
git branch -m chore/duro-1.50.50
$HOOK_CHECK &> /dev/null || exit 1

printf "Should throw when a non-chore branch carries dots\n"
git branch -m feat/duro-1.50.50
$HOOK_CHECK &> /dev/null && exit 1

printf "Should still throw without a type prefix\n"
git branch -m duro-1.50.50
$HOOK_CHECK &> /dev/null && exit 1

exit 0
