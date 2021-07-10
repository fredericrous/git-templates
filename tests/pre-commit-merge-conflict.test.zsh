#!/bin/zsh
TEST_NAME=`basename "$0"`
HOOK_CHECK=`echo ../../templates/hooks/$TEST_NAME | sed 's@\.test@@'`

printf "Should throw when file in merge state is detected\n"
TEST_FILE="merge-test"
cat <<EOL > $TEST_FILE
<<<<<<< HEAD
test
=======
new test
>>>>>>> refs/heads/nothing
EOL
git add $TEST_FILE
$HOOK_CHECK &> /dev/null && exit 1

printf "Should pass when no merge state detected\n"
echo "test" > $TEST_FILE
git add $TEST_FILE
$HOOK_CHECK &> /dev/null || exit 1

exit 0
