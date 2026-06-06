#!/bin/zsh
TEST_NAME=`basename "$0"`
HOOK_CHECK=`echo ${0:a:h}/../templates/hooks/$TEST_NAME | sed 's@\.test.zsh@@'`

printf "Should throw when subject is superior to 72 characters\n"
COMMIT_MSG="feat: this is a commit subject line that is definitely larger than the limit"
STD_RESULT=$($HOOK_CHECK <(echo $COMMIT_MSG) 2>&1)
echo $STD_RESULT | grep "✓" &> /dev/null && exit 1

printf "Should pass when subject is inferior to 72 chars and description short\n"
COMMIT_MSG="feat: a perfectly fine short description"
STD_RESULT=$($HOOK_CHECK <(echo $COMMIT_MSG) 2>&1)
echo $STD_RESULT | grep "✓" &> /dev/null || exit 1

printf "Should throw when description after the colon exceeds 50 chars\n"
# Subject is <= 72 overall, but the part after ': ' is longer than 50.
COMMIT_MSG="feat: this commit description is intentionally over fifty chars"
STD_RESULT=$($HOOK_CHECK <(echo $COMMIT_MSG) 2>&1)
echo $STD_RESULT | grep "description after" &> /dev/null || exit 1

printf "Should pass with a long scope as long as description stays under 50\n"
COMMIT_MSG="feat(a-fairly-long-scope-name): still a short description"
STD_RESULT=$($HOOK_CHECK <(echo $COMMIT_MSG) 2>&1)
echo $STD_RESULT | grep "Description size is at most" &> /dev/null || exit 1

printf "Should throw when no prefix\n"
COMMIT_MSG="no prefix"
STD_RESULT=$($HOOK_CHECK <(echo $COMMIT_MSG) 2>&1)
echo $STD_RESULT | grep "MUST be prefixed" &> /dev/null || exit 1

printf "Should pass when prefix\n"
COMMIT_MSG="feat: pass"
STD_RESULT=$($HOOK_CHECK <(echo $COMMIT_MSG) 2>&1)
echo $STD_RESULT | grep "A prefix is defined" &> /dev/null || exit 1

printf "Should pass when prefix with emoji\n"
COMMIT_MSG="👷  feat: pass"
STD_RESULT=$($HOOK_CHECK <(echo $COMMIT_MSG) 2>&1)
echo $STD_RESULT | grep "A prefix is defined" &> /dev/null || exit 1

printf "Should pass when prefix with scope\n"
COMMIT_MSG="feat(frontend): pass"
STD_RESULT=$($HOOK_CHECK <(echo $COMMIT_MSG) 2>&1)
echo $STD_RESULT | grep "A prefix is defined" &> /dev/null || exit 1

printf "Should pass when prefix with hyphenated scope\n"
COMMIT_MSG="fix(trade-agents): pass"
STD_RESULT=$($HOOK_CHECK <(echo $COMMIT_MSG) 2>&1)
echo $STD_RESULT | grep "A prefix is defined" &> /dev/null || exit 1

printf "Should not promote body-quoted conv-commit to subject\n"
# Body quotes another commit's conv-commit string; the FIRST line is the
# real subject (also conventional). The hook used to anchor anywhere via
# /ms regex flags and rewrote the commit using the body's quote.
cat <<EOL > COMMIT_MSG
revert: undo last change
In abc1234 ("fix(scope): something else") an unrelated diff slipped
in. Restoring the original value.
EOL
$HOOK_CHECK COMMIT_MSG &> /dev/null
sed '1!d' COMMIT_MSG | grep -E "revert: undo last change|⏪️.*revert: undo last change" &> /dev/null || exit 1

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
