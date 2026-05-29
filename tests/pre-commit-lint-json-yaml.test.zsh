#!/bin/zsh
TEST_NAME=`basename "$0"`
HOOK_CHECK=`echo ${0:a:h}/../templates/hooks/$TEST_NAME | sed 's@\.test@@'`

printf "Should throw on invalid JSON (missing comma)\n"
TEST_FILE="test.json"
cat <<EOL > $TEST_FILE
{
    "a": "json"
    "b": "json"
}
EOL
git add $TEST_FILE
$HOOK_CHECK &> /dev/null && exit 1

printf "Should pass valid JSON\n"
cat <<EOL > $TEST_FILE
{
    "0": 0
}
EOL
git add $TEST_FILE
$HOOK_CHECK &> /dev/null || exit 1

printf "Should throw on invalid YAML (tab indentation)\n"
git reset -q
rm -f test.json
TEST_FILE="test.yaml"
printf 'a:\n\tb: c\n' > $TEST_FILE
git add $TEST_FILE
$HOOK_CHECK &> /dev/null && exit 1

printf "Should pass valid YAML\n"
printf 'a:\n  b: c\n' > $TEST_FILE
git add $TEST_FILE
$HOOK_CHECK &> /dev/null || exit 1

exit 0
