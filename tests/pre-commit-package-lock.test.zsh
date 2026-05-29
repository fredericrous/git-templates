#!/bin/zsh
TEST_NAME=`basename "$0"`
HOOK_CHECK=`echo ${0:a:h}/../templates/hooks/$TEST_NAME | sed 's@\.test@@'`

# Each case starts from a clean index + working tree (the repo is a throwaway).
reset() {
    git reset -q 2>/dev/null
    rm -rf package.json package-lock.json sub
}

printf "Should throw when package-lock.json is staged alone\n"
reset
echo "{}" > package-lock.json
git add package-lock.json
$HOOK_CHECK &> /dev/null && exit 1

printf "Should pass when package.json and package-lock.json are both staged\n"
reset
echo "{}" > package.json
echo "{}" > package-lock.json
git add package.json package-lock.json
$HOOK_CHECK &> /dev/null || exit 1

printf "Should pass for a package.json with no sibling lockfile (non-npm marker)\n"
reset
echo '{"type":"commonjs"}' > package.json
git add package.json
$HOOK_CHECK &> /dev/null || exit 1

printf "Should pass per-directory: subdir package.json, lockfile only at root\n"
reset
mkdir -p sub
echo "{}" > package.json
echo "{}" > package-lock.json
echo '{"type":"commonjs"}' > sub/package.json
git add sub/package.json
$HOOK_CHECK &> /dev/null || exit 1

exit 0
