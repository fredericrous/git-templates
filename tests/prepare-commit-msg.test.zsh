#!/bin/zsh
TEST_NAME=`basename "$0"`
HOOK_CHECK=`echo ${0:a:h}/../templates/hooks/$TEST_NAME | sed 's@\.test.zsh@@'`
printf "Should extract JIRA ID and append it to commit msg\n"
git branch -m "feat/JIRA-1234-description"
$HOOK_CHECK COMMIT_MSG magic
cat COMMIT_MSG | grep JIRA-1234 &> /dev/null || exit 1

printf "Should extract Kanbanize ID and append it to commit msg\n"
git branch -m "fix/1234-something"
$HOOK_CHECK COMMIT_MSG2 magic
cat COMMIT_MSG2 | grep "#id 1234" &> /dev/null || exit 1
