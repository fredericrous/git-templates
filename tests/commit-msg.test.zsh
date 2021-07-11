#!/bin/zsh
TEST_NAME=`basename "$0"`
HOOK_CHECK=`echo ${0:a:h}/../templates/hooks/$TEST_NAME | sed 's@\.test.zsh@@'`

printf "Should throw when summary is superior to 50 characters\n"
COMMIT_MSG="this is a commit summary that is larger than 50 characters"
STD_RESULT=$($HOOK_CHECK <(echo $COMMIT_MSG) 2>&1)
echo $STD_RESULT | grep "✓" &> /dev/null && exit 1

printf "Should pass when summary is inferior to 50 chars\n"
COMMIT_MSG="summary inferior to 50 characters"
STD_RESULT=$($HOOK_CHECK <(echo $COMMIT_MSG) 2>&1)
echo $STD_RESULT | grep "✓" &> /dev/null || exit 1

printf "Should throw when no prefix\n"
COMMIT_MSG="no prefix"
STD_RESULT=$($HOOK_CHECK <(echo $COMMIT_MSG) 2>&1)
echo $STD_RESULT | grep "MUST be prefixed" &> /dev/null || exit 1

printf "Should pass when prefix\n"
COMMIT_MSG="feat: pass"
STD_RESULT=$($HOOK_CHECK <(echo $COMMIT_MSG) 2>&1)
echo $STD_RESULT | grep "A prefix is defined" &> /dev/null || exit 1

printf "Should pass when prefix with emoji\n"
COMMIT_MSG="👷  build: this is some summary"
STD_RESULT=$($HOOK_CHECK <(echo $COMMIT_MSG) 2>&1)
echo $STD_RESULT | grep "A prefix is defined" &> /dev/null || exit 1

printf "Should pass when prefix with scope\n"
COMMIT_MSG="feat(frontend): pass"
STD_RESULT=$($HOOK_CHECK <(echo $COMMIT_MSG) 2>&1)
echo $STD_RESULT | grep "A prefix is defined" &> /dev/null || exit 1

printf "Should throw when prefix but no description\n"
COMMIT_MSG="feat:"
STD_RESULT=$($HOOK_CHECK <(echo $COMMIT_MSG) 2>&1)
echo $STD_RESULT | grep "A description is present" &> /dev/null && exit 1

printf "Should add emoji\n"
echo "feat: pass" > COMMIT_MSG
$HOOK_CHECK COMMIT_MSG &> /dev/null
cat COMMIT_MSG | grep "✨  feat: pass" &> /dev/null || exit 1

printf "Should have blank line before and after body and end with new line\n"
cat <<EOL > COMMIT_MSG
feat: pass
some body you know
Signed-Off: me <my@self.test>
EOL
$HOOK_CHECK COMMIT_MSG &> /dev/null
wc -l COMMIT_MSG | grep "5" &> /dev/null || exit 1

printf "Should group footer\n"
cat <<EOL > COMMIT_MSG
feat: pass
some body you know
Signed-Off: me <my@self.test>

Co-authored-by: you <r@self.test>
EOL
$HOOK_CHECK COMMIT_MSG &> /dev/null
sed '5!d' COMMIT_MSG | grep "Signed-Off: me <my@self.test>" &> /dev/null
sed '6!d' COMMIT_MSG | grep "Co-authored-by: you <r@self.test>" &> /dev/null

exit 0
