#!/bin/zsh
# The pre-push DISPATCHER — deliberately NOT the same shape as pre-commit.
# It runs serially and stops at the first failure, because its steps are
# ordered and expensive: check the branch name, then rebase, then run the whole
# test suite. There is no point running tests after a rebase conflict.
#
# A shared "run all sub-hooks" helper is the obvious way to collapse the two
# dispatchers into one behaviour. These cases exist to make that break loudly.
TEST_NAME=`basename "$0"`
SHIM=${0:a:h}/../templates/hooks/pre-push

new_hooks_dir() {
    rm -rf "$1"; mkdir -p "$1"
    cp "$SHIM" "$1/pre-push"
    chmod +x "$1/pre-push"
}

printf "Should exit 0 when there are no sub-hooks\n"
new_hooks_dir p_empty
./p_empty/pre-push || exit 1

printf "Should run sub-hooks in glob order\n"
new_hooks_dir p_order
for n in aaa mmm zzz; do
    printf '#!/bin/sh\necho %s >> "$PWD/order.txt"\n' "$n" > p_order/pre-push-$n
    chmod +x p_order/pre-push-$n
done
rm -f order.txt
./p_order/pre-push || exit 1
[ "`tr -d '\n' < order.txt`" = "aaammmzzz" ] || { echo "  got: `cat order.txt`"; exit 1 }

# The difference from pre-commit, pinned: once one step fails the rest MUST NOT
# run. zzz would be the expensive test suite, and its preconditions are gone.
printf "Should stop at the FIRST failure and skip the rest\n"
new_hooks_dir p_stop
printf '#!/bin/sh\necho aaa >> "$PWD/stop.txt"\n'          > p_stop/pre-push-aaa
printf '#!/bin/sh\necho mmm >> "$PWD/stop.txt"\nexit 3\n'  > p_stop/pre-push-mmm
printf '#!/bin/sh\necho zzz >> "$PWD/stop.txt"\n'          > p_stop/pre-push-zzz
chmod +x p_stop/pre-push-*
rm -f stop.txt
./p_stop/pre-push; CODE=$?
[ $CODE -eq 3 ] || { echo "  exit was $CODE, expected the sub-hook's 3"; exit 1 }
[ "`tr -d '\n' < stop.txt`" = "aaammm" ] || { echo "  got: `cat stop.txt` — zzz should not have run"; exit 1 }

# Singular message, naming just the one that failed — distinct from
# pre-commit's "Error raised by:" list.
printf "Should name the single failing hook\n"
OUT=`./p_stop/pre-push 2>&1` && exit 1
echo "$OUT" | grep -q "Error raised by hook" || { echo "  wrong format: $OUT"; exit 1 }
echo "$OUT" | grep -q "pre-push-mmm"         || { echo "  hook not named: $OUT"; exit 1 }

printf "Should honour hook.skip as a SUBSTRING match\n"
new_hooks_dir p_skip
printf '#!/bin/sh\necho kept >> "$PWD/pskip.txt"\n'      > p_skip/pre-push-kept
printf '#!/bin/sh\necho gone >> "$PWD/pskip.txt"\nexit 1\n' > p_skip/pre-push-branch-pattern
chmod +x p_skip/pre-push-*
rm -f pskip.txt
git config --add hook.skip branch-pattern
./p_skip/pre-push || exit 1
git config --unset-all hook.skip
[ "`cat pskip.txt`" = "kept" ] || { echo "  got: `cat pskip.txt`"; exit 1 }

# git feeds pre-push the pushed refs on stdin; pre-push-run-tests-js reads them.
# A dispatcher that consumed stdin itself would starve the sub-hook, and the
# symptom would be "tests mysteriously never run on push".
printf "Should pass stdin through to sub-hooks\n"
new_hooks_dir p_stdin
printf '#!/bin/sh\ncat > "$PWD/stdin.txt"\n' > p_stdin/pre-push-reader
chmod +x p_stdin/pre-push-reader
rm -f stdin.txt
echo "refs/heads/main abc refs/heads/main def" | ./p_stdin/pre-push origin git@example.com || exit 1
grep -q "refs/heads/main" stdin.txt || { echo "  stdin lost: `cat stdin.txt 2>/dev/null`"; exit 1 }

printf "Should pass its arguments through to sub-hooks\n"
new_hooks_dir p_args
printf '#!/bin/sh\necho "$@" > "$PWD/pargs.txt"\n' > p_args/pre-push-echo
chmod +x p_args/pre-push-echo
./p_args/pre-push origin git@example.com < /dev/null || exit 1
[ "`cat pargs.txt`" = "origin git@example.com" ] || { echo "  got: `cat pargs.txt`"; exit 1 }

exit 0
