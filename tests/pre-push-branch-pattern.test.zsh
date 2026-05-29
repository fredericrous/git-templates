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

exit 0
