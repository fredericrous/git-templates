#!/bin/zsh
# Lint staged Kubernetes manifests with kube-linter, auto-discovering every
# .kube-linter*.yaml config in the repo root and running them in sequence.
# Author: https://github.com/fredericrous
ERROR_SIGN="  [38;5;160m✗[0m"
VALID_SIGN="  [38;5;112m✓[0m"
WARNING_SIGN="  [38;5;208m![0m"

# Trigger only when staged paths look kubernetes-ish. Conservative pattern —
# covers ``kubernetes/...``, ``manifests/...``, any ``charts/`` etc.; bail
# quickly when the change set doesn't touch k8s.
TRIGGERS=`git diff --diff-filter=d --cached --name-only | grep -E '^(kubernetes|manifests|charts?|k8s|helm|deploy)/.*\.ya?ml$'`
[ ${#TRIGGERS} -lt 1 ] && exit 0

# Tool gate
if ! type kube-linter > /dev/null; then
    printf "$WARNING_SIGN Kubernetes manifests staged. To lint them, install [38;5;208mkube-linter[0m\n"
    exit 0
fi

# Config gate — auto-discover every .kube-linter*.yaml at repo root.
# Glob expands to LITERAL pattern when no match (zsh default), so guard.
typeset -a CONFIGS
setopt local_options null_glob
CONFIGS=( .kube-linter*.yaml .kube-linter*.yml )
if [ ${#CONFIGS} -lt 1 ]; then
    printf "$WARNING_SIGN Kubernetes manifests staged but no .kube-linter*.yaml found; skipping\n"
    exit 0
fi

# Run kube-linter once per config. Each config's own ``excludes:`` and
# scope (set inside the YAML, not on the CLI) decide which manifests it
# applies to — so apps-vs-infra splits like the homelab's work without
# per-hook wiring.
OVERALL=0
for cfg in $CONFIGS; do
    kube-linter lint . --config "$cfg"
    rc=$?
    if [ $rc -ne 0 ]; then
        printf "$ERROR_SIGN kube-linter ($cfg) found issues\n"
        OVERALL=1
    fi
done

if [ $OVERALL -ne 0 ]; then
    exit 1
fi
printf "$VALID_SIGN kube-linter passed (${#CONFIGS} config$([ ${#CONFIGS} -gt 1 ] && echo 's'))\n"
