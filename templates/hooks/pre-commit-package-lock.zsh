#!/bin/zsh
# test that package.lock is updated when package.json is
# Author: https://github.com/fredericrous
ERROR_SIGN=$'  \e[38;5;160m✗\e[0m'
WARNING_SIGN=$'  \e[38;5;208m!\e[0m'
VALID_SIGN=$'  \e[38;5;112m✓\e[0m'

FILES=`git diff --diff-filter=d --cached --name-only`
if [ `echo $FILES | egrep -c 'package.json|package.lock.json'` = "0" -o `echo $FILES | egrep -c 'package.json|package.lock.json'` = "2" ]; then
    printf "$VALID_SIGN package.json & package-lock.json look in sync\n"
    exit 0
elif [[ `echo $FILES | egrep -c 'package.json'` = "1" && `echo $FILES | egrep -c 'package.lock.json'` = "0" ]]; then
    printf "$WARNING_SIGN You commit \033[38;5;208mpackage.json\033[0m without \033[38;5;208mpackage.lock.json\033[0m."
    read "? Confirm (y/N) " < /dev/tty
    if [[ $REPLY =~ ^[Yy]$ ]]; then
        exit 0
    fi
fi
printf "$ERROR_SIGN commit should cointain BOTH \033[38;5;208mpackage.json\033[0m and \033[38;5;208mpackage.lock.json\033[0m\n"
exit 1
