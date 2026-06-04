#!/bin/zsh
TEST_NAME=`basename "$0"`
HOOK_CHECK=`echo ${0:a:h}/../templates/hooks/$TEST_NAME | sed 's@\.test@@'`

printf "Should throw on invalid JSON (missing comma)\n"
TEST_FILE="test.json"
cat <<EOL > $TEST_FILE
{
    "a": "json"
    "b": "json"
}
EOL
git add $TEST_FILE
$HOOK_CHECK &> /dev/null && exit 1

printf "Should pass valid JSON\n"
cat <<EOL > $TEST_FILE
{
    "0": 0
}
EOL
git add $TEST_FILE
$HOOK_CHECK &> /dev/null || exit 1

printf "Should throw on invalid YAML (tab indentation)\n"
git reset -q
rm -f test.json
TEST_FILE="test.yaml"
printf 'a:\n\tb: c\n' > $TEST_FILE
git add $TEST_FILE
$HOOK_CHECK &> /dev/null && exit 1

printf "Should pass valid YAML\n"
printf 'a:\n  b: c\n' > $TEST_FILE
git add $TEST_FILE
$HOOK_CHECK &> /dev/null || exit 1

# A Go block directive ({{- if }} … {{- end }}) is genuinely not valid YAML —
# yq rejects it (a scalar like `name: {{ .X }}` happens to parse, so it's no
# test of the skip).
TMPL=$'{{- if .Values.enabled }}\nkind: Deployment\nmetadata:\n  name: x\n{{- end }}\n'

printf "Should SKIP a Helm chart template (Go-template YAML, sibling Chart.yaml)\n"
git reset -q
rm -f test.yaml
mkdir -p mychart/templates
printf 'apiVersion: v2\nname: mychart\nversion: 0.1.0\n' > mychart/Chart.yaml
printf '%s' "$TMPL" > mychart/templates/deploy.yaml
git add mychart/Chart.yaml mychart/templates/deploy.yaml
$HOOK_CHECK &> /dev/null || exit 1

printf "Should still throw on Go-template YAML outside a chart\n"
git reset -q
rm -rf mychart
TEST_FILE="test.yaml"
printf '%s' "$TMPL" > $TEST_FILE
git add $TEST_FILE
$HOOK_CHECK &> /dev/null && exit 1

exit 0
