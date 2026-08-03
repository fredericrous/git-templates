# githooks

Git hooks that catch a bad commit before it exists, as a single Rust binary
with no runtime dependencies — and that can be taken back out in one command.

![githooks catching a commit and letting the fixed one through](assets/githooks-demo.gif)

## Start here

If you have never run this before, three pages are enough:

1. **[Installing and activating](install.md)** — get the binary, turn hooks on
   in one repository, and turn them off again.
2. **[The checks](checks.md)** — what the twenty built-ins do, and what each
   one needs before it fires. Most are inert in most repositories.
3. **[Opting out](opting-out.md)** — skip one check, downgrade a whole trigger,
   bypass a single commit, or remove the hooks entirely.

## What this is, and what it refuses to be

A hook manager runs code on your machine, on every commit, with your
credentials, reading every staged file, while nobody is watching. Most of the
design decisions recorded in these pages follow from taking that seriously
rather than from a feature comparison:

- The **commit path links no external crates** — `githooks` and
  `githooks-runtime` are std-only, and `scripts/check-no-deps.sh` fails a build
  that changes that. It is a strong default rather than a prohibition, and the
  script itself says when reopening it would be a legitimate call.
- A cloned repository's declared checks are **inert until trusted**. See
  [the trust model](trust.md).
- **Uninstall removes what install wrote and nothing else.** A hook somebody
  else put in `.git/hooks` is named and left alone.
- **Your uncommitted work is the thing that must never be lost.** The run modes
  that make that hard are documented in
  [index fidelity and run modes](index-fidelity-and-run-modes.md), including
  the decision not to use `git stash` for it.

## How the documentation is organised

The pages under **Using githooks** are for anybody who has installed it or is
deciding whether to.

The pages under **Design records** are for maintainers. They are the arguments
behind the current behaviour, written as decisions with their alternatives and
their incidents attached — several describe a state of the world that no longer
exists and say so at the top. They are kept because "why is it like this" is a
question that comes back, not because they are onboarding material. Do not
start there.
