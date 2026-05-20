#!/bin/zsh
# Lint staged YAML files with yamllint, scoped to a repo-local .yamllint config.
# Author: https://github.com/fredericrous
ERROR_SIGN="  [38;5;160m✗[0m"
VALID_SIGN="  [38;5;112m✓[0m"
WARNING_SIGN="  [38;5;208m![0m"

# Only operate on staged YAML; deletes (d) excluded.
FILES=`git diff --diff-filter=d --cached --name-only | grep -E '\.ya?ml$'`
[ ${#FILES} -lt 1 ] && exit 0

# Tool gate — soft-fail if yamllint absent (matches pre-commit-lint-json-yaml.zsh).
if ! type yamllint > /dev/null; then
    printf "$WARNING_SIGN YAML files detected. To strict-lint them, install [38;5;208myamllint[0m\n"
    exit 0
fi

# Config gate — yamllint's stock rules are too noisy to enforce generically;
# require an opt-in repo-local config (.yamllint.yaml, .yamllint.yml,
# or .yamllint). Skip silently otherwise.
CONFIG=""
for candidate in .yamllint.yaml .yamllint.yml .yamllint; do
    if [ -f "$candidate" ]; then
        CONFIG="$candidate"
        break
    fi
done
[ -z "$CONFIG" ] && exit 0

# Run yamllint with the repo's config; pass only the staged files we found.
# `printf ${FILES[*]}` expands the newline-separated zsh array to positional args.
yamllint -c "$CONFIG" `printf ${FILES[*]}`
EXIT_CODE=$?
if [ $EXIT_CODE -ne 0 ]; then
    printf "$ERROR_SIGN yamllint found issues. Please fix\n"
    exit 1
fi
printf "$VALID_SIGN yamllint passed\n"
