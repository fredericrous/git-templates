#!/bin/zsh
# Author: https://github.com/fredericrous
SCRIPT_PATH=`dirname $(realpath -s "$0")`
ERROR_SIGN="  \u001b[38;5;160m\u2717\u001b[0m"
VALID_SIGN="  \u001b[38;5;112m\u2713\u001b[0m"

function exec_test() {
    f=$1
    TEST_REPO="test-repo-$RANDOM"
    TEST_NAME=`basename $f`
    TEMPLATE_DIR="$TEST_REPO-template"
    git init -q $TEST_REPO --template $TEMPLATE_DIR &> /dev/null
    cd $TEST_REPO
    printf "- $TEST_NAME\n"
    "$f"
    EXIT_CODE=$?
    cd -
    rm -rf $TEST_REPO $TEMPLATE_DIR
    if [[ $EXIT_CODE -eq 0 ]]; then
        printf "$VALID_SIGN passed\n"
    else
        printf "\n🚨  Test failed at \u001b[38;5;208m$TEST_NAME\u001b[0m\n"
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
