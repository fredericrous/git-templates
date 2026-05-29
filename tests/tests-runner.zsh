#!/bin/zsh
# Author: https://github.com/fredericrous
SCRIPT_PATH=`dirname $(realpath -s "$0")`
ERROR_SIGN=$'  \e[38;5;160m✗\e[0m'
VALID_SIGN=$'  \e[38;5;112m✓\e[0m'

function exec_test() {
    f=$1
    TEST_REPO="test-repo-$RANDOM"
    TEST_NAME=`basename $f`
    TEMPLATE_DIR="$TEST_REPO-template"
    git init -q $TEST_REPO --template $TEMPLATE_DIR &> /dev/null
    # Hard guard: if the throwaway dir can't be entered we MUST abort, never run
    # the test in the parent repo — tests do destructive git ops (branch -m,
    # rewriting .git/HEAD) that would otherwise corrupt this checkout.
    cd "$TEST_REPO" || { printf "\n🚨  cannot enter $TEST_REPO; aborting\n"; exit 1; }
    printf "- $TEST_NAME\n"
    "$f"
    EXIT_CODE=$?
    cd -
    rm -rf $TEST_REPO $TEMPLATE_DIR
    if [[ $EXIT_CODE -eq 0 ]]; then
        printf "$VALID_SIGN passed\n"
    else
        printf "\n🚨  Test failed at \033[38;5;208m$TEST_NAME\033[0m\n"
        exit $EXIT_CODE
    fi
}

if [[ ! -z $1 ]]; then
    exec_test $SCRIPT_PATH/*$1*
else
    for f in "$SCRIPT_PATH"/*\.test\.*; do
        exec_test $f
    done
fi
