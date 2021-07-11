#!/bin/zsh
TEST_NAME=`basename "$0"`
HOOK_CHECK=`echo ${0:a:h}/../templates/hooks/$TEST_NAME | sed 's@\.test@@'`

printf "Should throw when same email but different name\n"
git config user.name "test all mighty"
git config user.email "test@domain.test"
touch data1
git add data1
git commit -m"feat: data1" &> /dev/null

git config user.name "test mighty"
git config user.email "test@domain.test"
$HOOK_CHECK | grep "first time" &> /dev/null || error 1

exit 0
