# Index fidelity, fixes, and run modes

Status: **specification**. Nothing here is built.

Read against `pre-commit`, `lefthook` and `husky`. Four of their ideas are worth
taking, one is worth refusing on the record, and the first is not a missing
feature at all — it is a correctness gap we have been describing as a trade-off.

---

## 1. We name the staged files and then read the unstaged ones

`staged_files()` asks the index for the path list, which is right:

```rust
git::stdout(&["diff", "--diff-filter=d", "--cached", "--name-only"])
```

Those paths are then handed to a tool that opens them **from the working tree**:

```rust
pub fn run(root: &str, argv: &[String], extra: &[String]) -> bool
// …cmd.args(extra).current_dir(root)   ← `extra` is the path list; the tool reads the file
```

So a partially-staged file is judged by content that is not being committed.
`git add -p` half of `a.js`, commit, and prettier reads the whole working-tree
file: it fails on lines you did not stage, or passes on lines you did.

**This is systemic, not local.** Of the fifteen pre-commit checks:

| | checks | how |
|---|---|---|
| reads the **tree** — affected | 11 | prettier, lint-js, lint-json-yaml, yamllint, ruff, pyright, argo-lint, kube-linter, kubeconform, cargo-fmt, clippy |
| reads the **index** — correct | 2 | `merge-conflict` (`git grep --cached`), `ban-terms` (`git show :<file>`) |
| reads no file content | 2 | `package-lock` (path names only), `usual-name` |

**We already have the technique.** `ban_terms` selects candidates with
`git diff --cached` and then reads each one with `git show :<file>` — the index
blob, never the tree. The two checks that get this right are the two that were
ported most carefully, which is a hint about the other eleven rather than a
coincidence.

That suggests a second possible fix, and it is worth saying why it is not the
one to take. `git show :<file>` is enough when a check only needs CONTENT, which
is why it works for `ban-terms`. It is not enough for a tool invoked on a path:
prettier resolves its config by walking up from the file, kubeconform needs the
kustomization directory around it, and ruff needs the file to sit where its
`pyproject.toml` can be found. Feeding those a temp file changes their answer.
The stash puts the right content at the right path, which is the only fix that
serves all eleven.

`rust_tools.rs:149` calls it out and then accepts it:

> Note this inspects the WORKING TREE, not the index, so a partially-staged file
> is judged by its unstaged form too. Same trade-off cargo fmt gives everyone;
> scoping it to staged paths would need the edition resolved by hand.

The first sentence is true of all eleven. The second is the mistake:
it is not a trade-off cargo fmt gives everyone, it is one `pre-commit` removes
for everyone. Their wording is worth quoting because it names both failure
directions:

> Running hooks on unstaged changes can lead to both false-positives and
> false-negatives during committing. pre-commit only runs on the staged contents
> of files by temporarily stashing the unstaged changes while running hooks.

### The design

A guard around the whole pre-commit stage, not per check:

```rust
/// Unstaged changes, set aside for the duration of the stage.
struct StagedOnly { stash: Option<StashRef> }

impl StagedOnly {
    fn enter() -> Result<StagedOnly, String>;   // stash --keep-index, if there is anything to stash
}
impl Drop for StagedOnly { /* restore, ALWAYS */ }
```

Per-check stashing is wrong: twenty checks run concurrently and would fight over
one working tree. It belongs in `run_stage`, around the whole fan-out.

### The danger, stated plainly

**A stash that is taken and not restored loses uncommitted work.** That is a
worse failure than any this repository has had, including the two that
overwrote tracked files, because there is nothing on disk to recover from.

Rules, all of which want tests:

- **Nothing to stash → do nothing.** The common case must not touch the tree.
- **Restore in `Drop`**, so a panicking check (which we now catch — #64) and an
  early return both restore. `Drop` runs on unwind.
- **Restore failure is fatal and loud**: print the stash ref, do not swallow it,
  block the commit. A silent failure here is unrecoverable; a noisy one leaves
  `git stash list` holding the work.
- **Never stash when the tree is already mid-operation** — merge, rebase,
  cherry-pick. This is why §3 lands first: the state predicate it introduces is
  the precondition for this feature.
- **Conflicted paths abort the stage** rather than being stashed around.

### Reproduced

```
$ git show :x.json      # staged:      {"a": 2}       ← valid
$ cat x.json            # working tree: { THIS IS NOT JSON
$ githooks pre-commit
  ✗ Invalid JSON: x.json
  🚨 Error raised by: pre-commit-lint-json-yaml
```

The commit that was about to be made is valid. The hook blocked it anyway.

### Open question

Whether `cargo fmt` joins this. Its scope is a crate, not a file list, so
stashing fixes it for free — but it is also the check whose comment claimed the
trade-off was inherent, and that claim should be deleted either way.

---

## 2. `stage_fixed` — a formatter that fixes should re-stage

From lefthook's job options: *"automatically add modified files back to git
staging"*.

Three of our checks currently print an instruction and stop:

```
✗ Prettier found unformatted files. Run prettier --write on:
```

You then run the command yourself and commit again. lefthook's users run
`prettier --write` in the hook and get the result staged.

**This depends on §1 and must not ship before it.** Without the stash, "re-stage
what the formatter touched" re-stages unstaged work the author deliberately kept
back. With the stash in place, the tree contains exactly the staged content, so
anything the formatter changed is by definition part of this commit.

### The design

Opt-in per check, declared, not global:

```rust
pub enum Fix {
    /// Reports only. Every check today.
    None,
    /// A command that rewrites files, and whose result should be staged.
    Rewrite { argv: fn(&Ctx) -> Vec<String> },
}
```

Enabled by config, off by default — `git config githooks.fix true` — because a
hook that edits your files without being asked is a bigger surprise than one
that complains. Reported as a new `Outcome::Fixed`, which is neither `Passed`
(something happened) nor `Failed` (the commit proceeds).

Candidates: prettier (`--write`), ruff (`format` + `check --fix`), cargo fmt.
Not eslint `--fix`: its fixes are semantic and occasionally wrong.

---

## 3. Declared skip conditions, replacing one ad-hoc guard

lefthook:

```yml
pre-commit:
  commands:
    lint:
      skip: [merge, rebase]
```

and `skip: {ref: main}`, and `skip: {run: test "$NO_HOOK" -eq 1}`.

We have exactly one of these, hard-coded, in one dispatcher:

```rust
// dispatch.rs
if cherry_pick_in_progress(ctx.hooks_dir) { return Verdict::Proceed; }
// …and, twelve lines down:
// NB: no CHERRY_PICK_HEAD check here — the zsh pre-push had none either.
```

That comment is an admission: pre-push has no such guard because the shell
version had none, which is history rather than a decision.

### The design

`Scope` already declares *when a check applies to files*. This is the missing
half — when it applies to **repository state**:

```rust
pub struct Scope {
    pub files: &'static [&'static str],
    pub opt_in: &'static [&'static str],
    /// Git operations during which this check does not run.
    pub not_during: &'static [GitState],   // Merge | Rebase | CherryPick | Revert | Bisect
}
```

Detected from the files git writes into `$GIT_DIR`: `MERGE_HEAD`,
`REBASE_HEAD`/`rebase-merge/`, `CHERRY_PICK_HEAD`, `REVERT_HEAD`,
`BISECT_LOG`. `cherry_pick_in_progress` becomes one arm of that, and its hard-won
comment about `parent()` being lexical while `join("..")` is not moves with it.

**Only `not_during`, not lefthook's full set.** `ref:` conditions duplicate
`hook.skip`, which is already per-repo and already visible in the dashboard;
`run:` conditions are a shell escape hatch in a design that has deliberately
refused shells (see `.githooks.conf`). Taking the useful third is not a failure
to copy the other two.

---

## 4. `githooks run [--all-files]`

`pre-commit run --all-files` runs every hook over the whole repository rather
than the staged set. Two uses, both of which we currently cannot serve:

- **Adopting a check in an existing repo** — you want to know how big the mess
  is before you turn it on, and `git add .` is not an acceptable way to find out.
- **CI parity** — running the same checks in CI over the whole tree.

We have `githooks list` (would it run here?) and `githooks <check>` (run one,
staged). We have no "run everything, over everything".

`Scope::matches` already answers against an arbitrary path list, so the file
selection is done. The work is:

```
githooks run                 # every applicable check, staged files (what a commit does)
githooks run --all-files     # …over `git ls-files` instead
githooks run <check>         # one check, either way
```

`--all-files` skips the §1 stash: there is no staged/unstaged distinction to
protect when the answer is "all of it".

---

## 5. Shebang detection

pre-commit classifies files with `identify`, which reads shebangs, so an
extensionless `scripts/deploy` starting `#!/bin/sh` is a shell file.

Our `Scope.files` is suffix-only, and `.githooks.conf`'s `*.sh` inherits that —
a repository whose scripts have no extension cannot scope an external check onto
them at all.

Smallest useful version: `Scope.files` accepts a `#!` pattern.

```rust
Scope::files(&[".sh", "#!/bin/sh", "#!/usr/bin/env bash"])
```

Reading file heads costs an open per extensionless staged file, so it happens
only when a scope actually asks for a shebang, and only for files with no
matching suffix.

**Lowest value on this list.** Listed because it is the gap our own manifest
format inherits, not because anything is waiting on it.

---

## What we are not taking, and why

**`core.hooksPath` (husky).** Husky sets one config key and ships no per-repo
hook files. Our entire drift model — `githooks-fleet apply`, `BakeState`, the
`SHIMS` column, `recover_baked` — exists because we copy four files into
ninety-six repositories. A global `core.hooksPath` deletes that problem class
outright.

Refused, and the reason matters more than the refusal: `core.hooksPath` is
all-or-nothing per repository. "Managed vs unmanaged", which the fleet view is
built around, becomes unexpressible; a repository with hooks of its own silently
loses them; and a colleague who has never heard of this tool can read
`.git/hooks/pre-commit` and see what runs. That legibility is worth four files.

**repo + rev pinning, per-hook language isolation, `autoupdate` (pre-commit).**
These solve distributing hooks to strangers. We compile checks in and distribute
one binary through the fleet — the same problem, already solved differently.
Adopting the mechanism would mean adopting the problem.

**`remotes:` (lefthook).** The fleet's job.

**`piped:`, `priority:` (lefthook).** pre-commit runs concurrently and reports
every failure; pre-push runs serially and stops at the first. Those two shapes
are load-bearing and documented as such. A configurable ordering invites a third
shape nobody has asked for.

---

## Order

**PR 1 — `githooks run [--all-files]`.** Small, useful immediately, no risk, and
it gives the later work a way to be exercised over a whole repository.

**PR 2 — `not_during` git-state conditions.** Generalises the `CHERRY_PICK_HEAD`
special case and closes the pre-push gap. Independently useful, and §1 needs its
predicate.

**PR 3 — index fidelity.** The correctness gap. Alone, no fixing.

**PR 4 — `stage_fixed`.** Only after 3 is proven in the fleet for a while: this
is the first feature that would write to someone's index, and it should not be
the change that also introduces the stash.

Shebang detection is unscheduled.

## Open decisions

1. Does `--all-files` imply `--no-stash`, or is it an error to combine them?
2. Should `Outcome::Fixed` block a `pre-push`? It cannot occur there today, but
   the type would permit it.
3. Does index fidelity apply to `commit-msg` and `prepare-commit-msg`? They read
   a message file, not the tree, so probably not — but "probably" is how the
   pre-push cherry-pick gap started.
