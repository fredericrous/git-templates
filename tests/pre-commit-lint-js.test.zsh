#!/bin/zsh
TEST_NAME=`basename "$0"`
HOOK_CHECK=`echo ${0:a:h}/../templates/hooks/$TEST_NAME | sed 's@\.test@@'`

cat <<EOL > .eslintrc.js
module.exports = {
    root: true,
    extends: ['eslint:recommended'],
    env: {
        node: true,
        es6: true,
    },
    parserOptions: {
        ecmaVersion: 2021,
    },
}
EOL

printf "Should throw eslint error\n"
TEST_FILE="not-javascript.js"
echo "not javascript" > $TEST_FILE
git add $TEST_FILE
$HOOK_CHECK &> /dev/null && exit 1

printf "Should pass eslint\n"
cat <<EOL > $TEST_FILE
const add = (n1, n2) => (n1 + n2);
add(1,2);
EOL
git add $TEST_FILE
$HOOK_CHECK &> /dev/null || exit 1

exit 0
