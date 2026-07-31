# Custom checks — `.githooks.conf`

A repository can declare checks of its own. Put a `.githooks.conf` at the
repository root, **commit it**, and the hooks run it alongside the built-ins.

```
# stage       name        scope         severity  command
pre-commit    shellcheck  *.sh          block     scripts/lint-shell.sh
pre-commit    protos      *.proto,*.pb  block     buf lint
pre-push      smoke       *             warn      make smoke
```

Five whitespace-separated fields, in file order. Alignment is cosmetic; tabs and
single spaces parse identically.

## Why the file is committed

`.git/hooks` is not committed, which is why the old `pre-commit-*` filename
mechanism could never actually share a custom hook: every member of the team had
to install it by hand, and nothing told them when it changed. That mattered more
than the lexicographic ordering usually cited against filename prefixes.

A committed manifest is reviewed like any other change, arrives with a `git
pull`, and is visible in the fleet dashboard.

## The fields

**stage** — `pre-commit` or `pre-push`.

**name** — how you refer to the check in `hook.skip` and in a severity override.
It may not be the name of a built-in; see *What a repository cannot do*.

**scope** — `*` for every change, or a comma-separated list of `*.<ext>`.
Evaluated against the files staged for a commit, or against the range being
pushed. This gate is real: a `*.sh` check does not run on a commit that touches
no shell.

**severity** — `block` fails the stage; `warn` runs the check, prints whatever it
prints, and lets the commit through. It is your choice, per check.

**command** — the rest of the line, split on whitespace and executed directly
from the repository root.

## There is no shell

No pipes, no redirection, no globbing, no quoting. `make smoke` works; `find . |
xargs foo` does not — put that in a script and invoke the script.

Two reasons, and the second decided it. Windows has no `sh`, and every emulation
of one this project has tried has been a source of bugs. And a manifest line that
silently gained shell semantics would be a much larger thing to have introduced
than it looks.

## Exit codes

`0` passes. Anything else fails, and whether that stops you depends on the
severity column.

A command that **cannot be started at all** — a typo'd path, a tool nobody
installed — is neither. It is reported as a gap:

```
⚠ .githooks.conf: shellcheck could not run scripts/lint-shel.sh — No such file
⚠ 1 check(s) could not run: shellcheck
```

It does not block, because a command that never ran has not judged anything;
reporting it as a lint failure sends someone hunting for an error that does not
exist. But it is never silent, because a check that has quietly never executed is
the one failure this whole design is arranged against.

## A line nobody can parse is not skipped

The same rule applies to the manifest itself. A malformed line still produces a
check — one that reports on every commit and says which line and what was wrong:

```
⚠ .githooks.conf: oops — line 3: severity "LOUD" must be `block` or `warn`
```

Silently ignoring it would mean a check somebody committed months ago has never
run once and nothing ever said so.

## What a repository cannot do

**Take a built-in's name.** The line is refused. An external that could call
itself `pre-push-branch-protect` would either shadow the built-in or silently
lose to it, and a text file should not be able to do either.

**Declare the same name twice.** The second is refused: it could not be addressed
by `hook.skip` or by a severity override, so it would run anonymously.

**Run before the built-ins.** Externals are appended to each stage, always. A
third-party command must not be able to delay `pre-push-branch-protect`, and
appending is the only arrangement in which it cannot. On `pre-push`, which stops
at the first blocking failure, that means a built-in failure means your check
does not get a turn.

## A manifest is inert until you trust it

A repository you cloned can declare checks, and running them is a decision you
make — not one `git clone` makes for you. So nothing in `.githooks.conf` runs
until somebody accepts it here:

```sh
githooks trust            # shows what it declares, then records it
githooks trust --show     # what it declares, and whether it is trusted
githooks trust --revoke
```

`githooks install` asks once, with the declarations in view. Declining still
installs the built-ins.

Until then the checks are **reported, not dropped** — the point is that you can
see there is a decision waiting:

```
⚠ .githooks.conf: shellcheck — declared in an untrusted .githooks.conf …
⚠ 1 check(s) could not run: shellcheck
```

Acceptance is recorded against the file's CONTENT (`git hash-object`, which you
can run yourself), so a `git pull` that adds a command does not inherit the
trust you gave the file before it. When that happens the message says so —
`changed since it was trusted` — because "somebody edited this" is a different
thing to be told than "you have not looked at this yet".

**This is a floor, not a ceiling.** A built-in check still runs your
repository's own toolchain: `prettier` and `eslint` are taken from
`node_modules/.bin` when present, so a hostile `node_modules` needs no manifest
at all. That is the same exposure `npm install` already carries.

## Turning one off

Exactly as for a built-in:

```sh
git config hook.skip shellcheck                 # do not run it
git config githooks.severity.shellcheck warn    # run it, do not let it block
```

Prefer the second. `hook.skip` matches by substring — `hook.skip = e` disables
every check — and it removes the signal along with the block. A downgrade keeps
the check running and reporting.

## Seeing what you declared

```sh
githooks list
```

```
pre-commit
  ● pre-commit-merge-conflict
  ○ pre-commit-clippy               inert here — needs .rs + Cargo.toml
  ● shellcheck (declared)
  ✗ oops (declared)                 .githooks.conf line 3: severity "LOUD" …
pre-push
  ● pre-push-branch-protect
  ● smoke (declared)

  ● runs here   ○ inert   ⊘ skipped via hook.skip   ✗ declaration unusable
```

Across the fleet, `githooks-fleet` has a `DECL` column — `2` for two declared
checks, `2!1` when one of them cannot run — and lists them per repository in the
detail pane.

## Why this format and not TOML

TOML would be nicer to write and costs a dependency tree that would then run on
every commit in ninety-six repositories. For four fields and a command, twenty
lines of `std` parsing wins. See `scripts/check-no-deps.sh` for the reasoning
behind that default; it is a judgement about the commit path's supply chain, not
a prohibition, and a genuinely rich format would be worth reopening it for.
