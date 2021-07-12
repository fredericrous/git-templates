#!/bin/zsh
# Run tests before push
# Author: https://github.com/fredericrous
zero=$(git hash-object --stdin </dev/null | tr '[0-9a-f]' '0')

while read local_ref local_oid remote_ref remote_oid
do
    if test "$local_oid" = "$zero"
    then
        # Handle delete
        :
    else
        if test "$remote_oid" = "$zero"; then
            # New branch, examine all commits
            range="$local_oid"
        else
            # Update to existing branch, examine new commits
            range="$remote_oid..$local_oid"
        fi

        modified_files=$(git diff-tree --no-commit-id --name-only -r "$range")
        js_files=(`echo $modified_files | grep -E '\.(js|jsx|ts|tsx|vue)$'`)
        if [[ ${#js_files} -gt 0 ]]; then
            typeset -aU js_directories
            for file in $js_files; do
                js_directories+=($(dirname $file))
            done
            ALL_PKG_JSON=($(dirname $(fd package.json $(git rev-parse --show-toplevel))))
            if [[ $ALL_PKG_JSON = 1 ]]; then
                cd $(dirname $ALL_PKG_JSON)
                npm test && npm audit || exit 1
            else
                for i in $ALL_PKG_JSON; do
                    for folder in $js_directories; do
                        if test "${folder#*$i}" = "$folder"; then
                            cd $folder
                            npm test && npm audit || exit 1
                            break
                        fi
                    done
                done
            fi
        fi
    fi
done

exit 0
