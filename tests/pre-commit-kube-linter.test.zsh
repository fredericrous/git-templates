#!/bin/zsh
# kube-linter had no tests. Covers the hook's own logic: path scoping, the tool
# gate, and the CONFIG gate (no .kube-linter*.yaml → warn and skip, never lint
# with stock rules). Must hold whether or not kube-linter is installed, since CI
# does not install it.
TEST_NAME=`basename "$0"`
HOOK=`echo ${0:a:h}/../templates/hooks/$TEST_NAME | sed 's@\.test@@'`
git config user.email t@t.test; git config user.name t
stage() { mkdir -p "$(dirname "$1")"; printf '%b' "$2" > "$1"; git add "$1"; }

printf "Should ignore YAML outside the k8s-ish path prefixes\n"
stage src/app.yaml 'kind: Deployment\n'
$HOOK > /tmp/kl.txt 2>&1 || exit 1
[ -s /tmp/kl.txt ] && { echo "  expected silence, got: $(cat /tmp/kl.txt)"; exit 1 }
git rm -q -f --cached src/app.yaml

# Stock kube-linter rules are too noisy to enforce generically, so a repo-local
# config is the opt-in. Without one the hook must SAY it skipped rather than
# lint anyway or fall silent.
printf "Should warn and skip when no .kube-linter config exists\n"
stage kubernetes/app/deploy.yaml 'apiVersion: apps/v1\nkind: Deployment\nmetadata:\n  name: d\n'
$HOOK > /tmp/kl.txt 2>&1 || exit 1
grep -qi "skipping\|install" /tmp/kl.txt || { echo "  expected a skip notice, got: $(cat /tmp/kl.txt)"; exit 1 }

printf "Should not block the commit when the tool or config is absent\n"
$HOOK > /dev/null 2>&1 || exit 1
git rm -q -f --cached kubernetes/app/deploy.yaml
exit 0
