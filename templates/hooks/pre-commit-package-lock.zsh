#!/bin/zsh
# A package.json and its sibling package-lock.json should be committed together.
#
# Checked PER DIRECTORY and only when a lockfile actually exists, so:
#   - a package.json that isn't a real npm project (no package-lock.json beside
#     it — e.g. a .git/hooks/package.json type-marker) never demands one;
#   - in a monorepo, one project's lockfile isn't satisfied by another's.
# Author: https://github.com/fredericrous
ERROR_SIGN=$'  \e[38;5;160m✗\e[0m'
WARNING_SIGN=$'  \e[38;5;208m!\e[0m'
VALID_SIGN=$'  \e[38;5;112m✓\e[0m'

staged=(${(f)"$(git diff --diff-filter=d --cached --name-only)"})

is_staged() { print -rl -- $staged | grep -qxF -- "$1" }
sibling()   { [[ $1 == . ]] && print -r -- "$2" || print -r -- "$1/$2" }

forgot_lock=()   # package.json staged; sibling lock exists on disk but unstaged
orphan_lock=()   # package-lock.json staged without its sibling package.json

for f in $staged; do
    dir=${f:h}
    case ${f:t} in
        package.json)
            lock=$(sibling $dir package-lock.json)
            is_staged "$lock" && continue          # both staged → in sync
            [[ -f $lock ]] && forgot_lock+=("$f")   # real npm project, lock unstaged
            ;;
        package-lock.json)
            pkg=$(sibling $dir package.json)
            is_staged "$pkg" || orphan_lock+=("$f")
            ;;
    esac
done

if (( ${#forgot_lock} + ${#orphan_lock} == 0 )); then
    printf "$VALID_SIGN package.json & package-lock.json look in sync\n"
    exit 0
fi

for f in $orphan_lock; do
    printf "$ERROR_SIGN \033[38;5;208m%s\033[0m staged without its package.json\n" "$f"
done
for f in $forgot_lock; do
    printf "$WARNING_SIGN \033[38;5;208m%s\033[0m changed but its package-lock.json is not staged\n" "$f"
done

# An orphan lock is a hard error. A merely-forgotten lock can be confirmed past
# — but only when a real terminal is attached; never crash on a missing /dev/tty
# (the old `read < /dev/tty` aborted non-interactive commits with "device not
# configured"). Headless callers can bypass with `git -c hook.skip=package-lock`.
if (( ${#orphan_lock} == 0 )) && { exec 3</dev/tty } 2>/dev/null; then
    read -u 3 "REPLY?  Commit anyway? (y/N) "
    exec 3<&-
    [[ $REPLY == [Yy]* ]] && exit 0
fi
printf "$ERROR_SIGN Run \033[38;5;208mnpm install\033[0m and stage the lockfile, or bypass with \033[38;5;208mgit -c hook.skip=package-lock commit\033[0m\n"
exit 1
