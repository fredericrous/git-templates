#!/bin/zsh
TEST_NAME=`basename "$0"`
HOOK_CHECK=`echo ${0:a:h}/../templates/hooks/$TEST_NAME | sed 's@\.test.zsh@.js@'`
TEST_FILE="describe.js"

# stage <content> — write it to the candidate file and index it.
stage() {
  printf '%s\n' "$1" > $TEST_FILE
  git add $TEST_FILE
}

# The hook exits non-zero when it finds a banned term.
expect_reject() {
  stage "$2"
  printf "Should reject: $1\n"
  $HOOK_CHECK &> /dev/null && exit 1
}

expect_accept() {
  stage "$2"
  printf "Should accept: $1\n"
  $HOOK_CHECK &> /dev/null || exit 1
}

# --- the original guarantees -------------------------------------------
expect_reject "fdescribe(" 'fdescribe('
expect_accept "plain describe(" 'describe('
expect_reject "a debugger statement" 'debugger;'
expect_reject "describe.skip" 'describe.skip("x", () => {})'
expect_reject "it.only" 'it.only("x", () => {})'
expect_reject "context.skip" 'context.skip("x", () => {})'

# --- exact identifiers, not prefixes -----------------------------------
# vitest's conditional variants are legitimate API, not disabled suites.
expect_accept "describe.skipIf" 'describe.skipIf(!process.env.CI)("x", () => {})'
expect_accept "it.skipIf" 'it.skipIf(cond)("x", () => {})'
expect_accept "describe.runIf" 'describe.runIf(cond)("x", () => {})'
expect_accept "a suffixed .only" 'it.onlyWhen(cond)("x", () => {})'
expect_accept "profit( is not fit(" 'const x = profit();'
expect_accept "obj.fit( is not fit(" 'layout.fit();'
expect_accept "debuggerUtils is not debugger" 'const debuggerUtils = 1;'

# --- comments are discussion, not code ---------------------------------
expect_accept "line comment naming a banned term" '// do not use describe.skip here'
expect_accept "block comment naming a banned term" '/* it.only is banned */'
expect_accept "jsdoc naming debugger" '/**
 * Never leave a debugger statement behind.
 */
const x = 1;'
expect_accept "comment naming fdescribe(" '// fdescribe( is banned'

# A comment must not mask real code elsewhere in the same file.
expect_reject "comment plus real violation" '// describe.skip is banned
describe.skip("x", () => {})'

# --- strings are not code ----------------------------------------------
expect_accept "term inside a string literal" 'const msg = "use describe.skip sparingly";'
expect_accept "term inside a template literal" 'const msg = `avoid it.only`;'

# --- the hook never flags its own source --------------------------------
# It defines every term it bans (`debugger` is a TERMS key), so editing it has
# to stay possible from both layouts it is run from.
printf "Should accept: the hook's own source\n"
mkdir -p templates/hooks
printf '%s\n' 'const TERMS = { debugger: "debugger;?" };' > templates/hooks/pre-commit-ban-terms.js
git add templates/hooks/pre-commit-ban-terms.js
$HOOK_CHECK &> /dev/null || exit 1
git rm -q -f --cached templates/hooks/pre-commit-ban-terms.js
rm -rf templates

# --- removing a violation is not committing one ------------------------
# -G matches removed lines as readily as added ones, so deleting a debugger
# used to be reported as introducing one.
stage 'debugger;'
git commit -q -m "test: seed a violation" --no-verify
expect_accept "removing a debugger line" 'const x = 1;'

exit 0
