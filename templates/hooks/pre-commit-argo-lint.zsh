#!/bin/zsh
# Semantic-validate staged Argo Workflow manifests with `argo lint --offline`.
# Catches what kubeconform's schema check can't: bad template references,
# undefined entrypoints, broken DAG dependencies, invalid task wiring.
# Only runs when the argo CLI is installed — otherwise warns and skips so
# the hook doesn't gate on every developer having the toolchain.
# Author: https://github.com/fredericrous
ERROR_SIGN=$'  \033[38;5;160m✗\033[0m'
VALID_SIGN=$'  \033[38;5;112m✓\033[0m'
WARNING_SIGN=$'  \033[38;5;208m!\033[0m'

# Only consider staged k8s-ish YAML — same prefix set as the kubeconform
# and kube-linter hooks.
STAGED=`git diff --diff-filter=d --cached --name-only | grep -E '^(kubernetes|manifests|charts?|k8s|helm|deploy)/.*\.ya?ml$'`
[ ${#STAGED} -lt 1 ] && exit 0

# Of those, keep only files that declare an Argo workflow kind. Anything
# else (Deployments, ConfigMaps, Helm values) is out of scope here.
typeset -a WF
for f in ${(f)STAGED}; do
    if grep -qE '^kind: (Workflow|CronWorkflow|WorkflowTemplate|ClusterWorkflowTemplate)$' "$f" 2>/dev/null; then
        WF+=("$f")
    fi
done
[ ${#WF} -lt 1 ] && exit 0

# argo CLI required. Soft-fail so a missing toolchain doesn't block the
# commit (the CI argo-lint job is the hard gate).
if ! type argo > /dev/null; then
    printf "$WARNING_SIGN Argo workflow manifests staged. Skipping argo lint; install: \033[38;5;208margo\033[0m\n"
    exit 0
fi

# --offline: no cluster needed; inline templates only (this is what the
# repo uses). Flux ${VAR} postBuild placeholders are inert strings to the
# linter; Argo {{...}} templating is what it actually checks.
OUT=`argo lint --offline ${WF} 2>&1`
if [ $? -ne 0 ]; then
    printf "$ERROR_SIGN argo lint failed:\n"
    print -r -- "$OUT"
    exit 1
fi
printf "$VALID_SIGN argo lint passed (${#WF} workflow manifest$([ ${#WF} -gt 1 ] && echo 's'))\n"
