#!/bin/zsh
TEST_NAME=`basename "$0"`
HOOK_CHECK=`echo ${0:a:h}/../templates/hooks/$TEST_NAME | sed 's@\.test@@'`

# ESLint 9+ uses flat config (eslint.config.js); the legacy .eslintrc.* format
# is ignored, so a stale .eslintrc.js made even valid JS fail with "couldn't
# find eslint.config.js". The temp test repo has no package.json, so this file
# is loaded as CommonJS.
cat <<EOL > eslint.config.js
module.exports = [
    { rules: { "no-unused-vars": "error" } },
]
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
