#!/bin/zsh
TEST_NAME=`basename "$0"`
HOOK_CHECK=`echo ${0:a:h}/../templates/hooks/$TEST_NAME | sed 's@\.test@@'`

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


# The scope change: a marker in a file this commit does NOT touch is not this
# commit's problem. Under the old whole-index grep, one bad tracked file blocked
# every commit in the repo until someone fixed it.
printf "Should ignore markers in a file this commit does not stage\n"
printf 'a\n<<<<<<< HEAD\nb\n=======\nc\n>>>>>>> other\n' > untouched.txt
git add untouched.txt
git -c core.hooksPath=/dev/null commit -q -m "seed a conflicted file" --no-verify
printf 'unrelated\n' > other.txt
git add other.txt
$HOOK_CHECK || exit 1          # the staged file is clean → pass
git rm -q -f --cached other.txt

printf "Should still catch a marker in a file this commit DOES stage\n"
printf 'x\n<<<<<<< HEAD\ny\n=======\nz\n>>>>>>> b\n' > staged-bad.txt
git add staged-bad.txt
$HOOK_CHECK && exit 1
git rm -q -f --cached staged-bad.txt
exit 0
