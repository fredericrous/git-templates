#!/bin/zsh
# kubeconform had no tests. Covers the hook's own logic: path scoping, the
# two-tool gate, and kustomization-root discovery (walking up from a staged
# file). Must hold with or without kustomize/kubeconform installed.
TEST_NAME=`basename "$0"`
HOOK=`echo ${0:a:h}/../templates/hooks/$TEST_NAME | sed 's@\.test@@'`
git config user.email t@t.test; git config user.name t
stage() { mkdir -p "$(dirname "$1")"; printf '%b' "$2" > "$1"; git add "$1"; }

printf "Should ignore YAML outside the k8s-ish path prefixes\n"
stage src/app.yaml 'kind: Deployment\n'
$HOOK > /tmp/kc.txt 2>&1 || exit 1
[ -s /tmp/kc.txt ] && { echo "  expected silence, got: $(cat /tmp/kc.txt)"; exit 1 }
git rm -q -f --cached src/app.yaml

# Raw-YAML validation is deliberately out of scope: a project either uses
# kustomize or it does not. A staged manifest with no kustomization.yaml
# anywhere above it must produce NOTHING.
# Root discovery happens AFTER the tool gate, so this case only means anything
# when both tools are present. Asserting silence unconditionally encoded the
# machine it was written on: CI has neither tool, hits the gate, and warns.
printf "Should stay silent when no kustomization root is above the file\n"
stage kubernetes/loose/deploy.yaml 'apiVersion: v1\nkind: ConfigMap\nmetadata:\n  name: c\n'
if type kustomize > /dev/null 2>&1 && type kubeconform > /dev/null 2>&1; then
    $HOOK > /tmp/kc.txt 2>&1 || exit 1
    [ -s /tmp/kc.txt ] && { echo "  expected silence, got: $(cat /tmp/kc.txt)"; exit 1 }
else
    printf "  ! kustomize/kubeconform unavailable — skipping\n"
fi
git rm -q -f --cached kubernetes/loose/deploy.yaml

# With a kustomization.yaml ABOVE the staged file, the walk-up must find it and
# the hook must act — lint it, or say it is skipping for a missing tool.
printf "Should find a kustomization root above the staged file\n"
stage kubernetes/app/base/kustomization.yaml 'resources:\n  - deploy.yaml\n'
stage kubernetes/app/base/deploy.yaml 'apiVersion: v1\nkind: ConfigMap\nmetadata:\n  name: c\n'
$HOOK > /tmp/kc.txt 2>&1
[ -s /tmp/kc.txt ] || { echo "  root above the file was not discovered"; exit 1 }
exit 0
