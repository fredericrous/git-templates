#!/bin/zsh
TEST_NAME=`basename "$0"`
HOOK_CHECK=`echo ../../templates/hooks/$TEST_NAME | sed 's@\.test.zsh@@'`.js

printf "Should throw when finds unwanted token fdescribe(\n"
TEST_FILE="describe.js"
echo "fdescribe(" > $TEST_FILE
git add $TEST_FILE
$HOOK_CHECK &> /dev/null && exit 1

printf "Should pass when no unwanted token found\n"
echo "describe(" > $TEST_FILE
git add $TEST_FILE
$HOOK_CHECK &> /dev/null || exit 1

exit 0
