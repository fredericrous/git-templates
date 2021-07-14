#!/bin/zsh
TEST_NAME=`basename "$0"`
HOOK_CHECK=`echo ${0:a:h}/../templates/hooks/$TEST_NAME | sed 's@\.test@@'`

# printf "Should throw when package-lock.json is staged alone\n"
TEST_FILE="package-lock.json"
echo "{}" > $TEST_FILE
git add $TEST_FILE
# $HOOK_CHECK &> /dev/null && exit 1

printf "Should pass when package.json and package-lock.json are both staged\n"
TEST_FILE="package.json"
echo "{}" > $TEST_FILE
git add $TEST_FILE
$HOOK_CHECK || exit 1

# printf "Should throw when package.json is staged alone\n"
# TEST_FILE="package-lock.json"
# git rm --cached $TEST_FILE &> /dev/null
# rm $TEST_FILE
# yes | $HOOK_CHECK &> /dev/null && exit 0

exit 0
