#!/bin/zsh
TEST_NAME=`basename "$0"`
HOOK_CHECK=`echo ${0:a:h}/../templates/hooks/$TEST_NAME | sed 's@\.test@@'`

# The hook only runs when the repo opts into prettier. Give the temp
# test repo a config so the gate passes; `prettier --check` then uses
# defaults (the empty config) — enough to tell formatted from not.
echo '{}' > .prettierrc

# Skip gracefully if prettier isn't resolvable in this environment
# (the hook itself warn+skips, so the assertions below would be
# meaningless). Mirrors the npx-availability assumption of the
# lint-js test.
if ! npx --no-install prettier --version > /dev/null 2>&1 \
   && ! type prettier > /dev/null 2>&1; then
    printf "  ! prettier unavailable — skipping\n"
    exit 0
fi

printf "Should pass on well-formatted file\n"
TEST_FILE="ok.ts"
printf 'export const a = 1;\n' > $TEST_FILE
git add $TEST_FILE
$HOOK_CHECK &> /dev/null || exit 1

printf "Should throw on a badly-formatted file\n"
# Bad: double-quotes prettier would single... no — default is double.
# Use indentation + missing semicolon + trailing spaces that prettier
# definitively rewrites.
printf 'export const a=1   \nconst b =2\n' > $TEST_FILE
git add $TEST_FILE
$HOOK_CHECK &> /dev/null && exit 1

printf "Should ignore files prettier doesn't handle\n"
git reset -q
rm -f ok.ts
TEST_FILE="data.bin"
printf 'not   formatted   at all\n' > $TEST_FILE
git add $TEST_FILE
# No prettier-handled files staged → hook exits 0 without running.
$HOOK_CHECK &> /dev/null || exit 1

exit 0
