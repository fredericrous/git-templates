#!/bin/zsh
TEST_NAME=`basename "$0"`
HOOK_CHECK=`echo ../../templates/hooks/$TEST_NAME | sed 's@\.test@@'`
CURRENT_BRANCH=`git branch --show-current`

printf "Should pass when no head\n"
$HOOK_CHECK &> /dev/null || exit 1

printf "Should pass when branch already on server\n"
echo "refs/heads/main" > .git/HEAD
$HOOK_CHECK &> /dev/null || exit 1

printf "Should throw when branch do not conform to criteria\n"
echo "refs/heads/not-here" > .git/HEAD
git branch -m "do-not-conform"
ERROR_CODE=0
$HOOK_CHECK &> /dev/null && ERROR_CODE=1
git branch -m $CURRENT_BRANCH
[[ $ERROR_CODE -eq 1 ]] && exit 1

printf "Should pass when branch conforms to creteria\n"
git branch -m "feat/0-test"
ERROR_CODE=0
$HOOK_CHECK &> /dev/null || ERROR_CODE=1
git branch -m $CURRENT_BRANCH
exit $ERROR_CODE
