#!/bin/zsh
TEST_NAME=`basename "$0"`
HOOK_CHECK=`echo ../../templates/hooks/$TEST_NAME | sed 's@\.test@@'`

printf "Should throw yq lint error\n"
TEST_FILE="test.json"
cat <<EOL > $TEST_FILE
{
    "not": "json"
    "not": "json"
}
EOL
git add $TEST_FILE
$HOOK_CHECK &> /dev/null && exit 1

printf "Should pass yq lint\n"
cat <<EOL > $TEST_FILE
{
    "0": 0
}
EOL
git add $TEST_FILE
$HOOK_CHECK &> /dev/null || exit 1

exit 0
