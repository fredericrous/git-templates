#!/bin/zsh
TEST_NAME=`basename "$0"`
HOOK_CHECK=`echo ${0:a:h}/../templates/hooks/$TEST_NAME | sed 's@\.test.zsh@@'`
echo $HOOK_CHECK
printf "Should extract JIRA ID and append it to commit msg\n"
git branch -m "feat/JIRA-1234-description"
$HOOK_CHECK COMMIT_MSG magic
cat COMMIT_MSG | grep JIRA-1234 &> /dev/null || exit 1

printf "Should extract Kanbanize ID and append it to commit msg\n"
git branch -m "fix/1234-something"
$HOOK_CHECK COMMIT_MSG2 magic
cat COMMIT_MSG2 | grep "#id 1234" &> /dev/null || exit 1

# Same two extractions through the grep fallback: rg speaks \d, grep -E needs
# [0-9], so these patterns are written twice and must stay in step.
printf "Should extract both ids when falling back to grep\n"
git branch -m "feat/JIRA-1234-description"
HOOKS_FORCE_GREP=1 $HOOK_CHECK COMMIT_MSG3 magic
cat COMMIT_MSG3 | grep JIRA-1234 &> /dev/null || exit 1
git branch -m "fix/1234-something"
HOOKS_FORCE_GREP=1 $HOOK_CHECK COMMIT_MSG4 magic
cat COMMIT_MSG4 | grep "#id 1234" &> /dev/null || exit 1
