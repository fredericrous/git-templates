#!/bin/zsh
# test that package.lock is updated when package.json is
# Author: https://github.com/fredericrous
WARNING_SIGN="  \u001b[38;5;208m!\u001b[0m"
VALID_SIGN="  \u001b[38;5;112m\u2713\u001b[0m"

FILES=`git diff --diff-filter=d --cached --name-only`
if [ `echo $FILES | egrep -c 'package.json|package.lock.json'` = "0" -o `echo $FILES | egrep -c 'package.json|package.lock.json'` = "2" ]; then
    printf "$VALID_SIGN package.json & package-lock.json look in sync\n"
    exit 0
elif [[ `echo $FILES | egrep -c 'package.json'` = "1" && `echo $FILES | egrep -c 'package.lock.json'` = "0" ]]; then
    printf "$WARNING_SIGN You commit \u001b[38;5;208mpackage.json\u001b[0m without \u001b[38;5;208mpackage.lock.json\u001b[0m."
    read "? Confirm (y/N) " < /dev/tty
    if [[ $REPLY =~ ^[Yy]$ ]]; then
        exit 0
    fi
fi
printf "$WARNING_SIGN commit should cointain BOTH \u001b[38;5;208mpackage.json\u001b[0m and \u001b[38;5;208mpackage.lock.json\u001b[0m\n"
printf "If you ran npm audit fix, this is expected, answer: y\n"
read "REPLY?Should we continue with the commit? Confirm (y/N) " < /dev/tty
if [[ $REPLY =~ ^[Yy]$ ]]; then
    exit 0
fi
exit 1
