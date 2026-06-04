#!/bin/zsh
# Lint staged JSON/YAML files for syntax errors before commit.
#   .json → node's JSON.parse (strict: catches the missing/trailing commas that
#           yq's lenient YAML-superset parser silently accepts).
#   .yaml → yq (the right tool for YAML).
# Like the rest of the hook set this leans on node; if a linter is absent we
# warn and skip rather than block the commit.
# Author: https://github.com/fredericrous
ERROR_SIGN=$'  \e[38;5;160m✗\e[0m'
VALID_SIGN=$'  \e[38;5;112m✓\e[0m'
WARNING_SIGN=$'  \e[38;5;208m!\e[0m'

staged=(${(f)"$(git diff --diff-filter=d --cached --name-only)"})
json_files=(${(M)staged:#*.json})
yaml_files=(${(M)staged:#*.yaml})

(( ${#json_files} + ${#yaml_files} == 0 )) && exit 0

rc=0

if (( ${#json_files} )); then
    if type node > /dev/null; then
        for f in $json_files; do
            node -e 'JSON.parse(require("fs").readFileSync(process.argv[1],"utf8"))' "$f" 2>/dev/null \
                || { printf "$ERROR_SIGN Invalid JSON: \033[38;5;208m%s\033[0m\n" "$f"; rc=1; }
        done
    else
        printf "$WARNING_SIGN JSON files detected. To lint them, install \033[38;5;208mnode\033[0m\n"
    fi
fi

if (( ${#yaml_files} )); then
    if type yq > /dev/null; then
        for f in $yaml_files; do
            # Helm chart templates carry Go templating ({{ }}) and aren't valid
            # YAML until Helm renders them, so yq can't lint them. Skip a staged
            # YAML that lives in a chart's templates/ dir (sibling Chart.yaml at
            # the chart root) — otherwise valid chart commits would need
            # --no-verify just to get past this hook.
            if [[ "$f" == */templates/* && -f "${f%/templates/*}/Chart.yaml" ]]; then
                continue
            fi
            yq e 'true' "$f" > /dev/null 2>&1 \
                || { printf "$ERROR_SIGN Invalid YAML: \033[38;5;208m%s\033[0m\n" "$f"; rc=1; }
        done
    else
        printf "$WARNING_SIGN YAML files detected. To lint them, install \033[38;5;208myq\033[0m\n"
    fi
fi

(( rc == 0 )) && printf "$VALID_SIGN Json/Yaml Lint passed\n"
exit $rc
