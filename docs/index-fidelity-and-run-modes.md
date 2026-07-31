# Index fidelity, fixes, and run modes

Status: **specification**. Nothing here is built.

Read against `pre-commit`, `lefthook` and `husky`, then re-read as somebody who
would have to get this through an adoption review. Four of their ideas are worth
taking, one is worth refusing on the record, the first is not a missing feature
at all — it is a correctness gap we have been describing as a trade-off — and
before any of them there is a trust problem that has nothing to do with the
three tools and everything to do with a decision we already shipped.

The last two sections are the ones an adoption review reads first: *What we are
not taking* and *What this does not solve*. The second is a list of honest
noes — pinned tool versions, CI enforcement, DCO — and it is deliberately not a
backlog.

---

## 0. A cloned repository can run its own commands

`.githooks.conf` is committed, which is the point: a team shares a check by
committing it. The consequence had not been written down.

`git clone` seeds `.git/hooks` from `init.templateDir`, so a fresh clone arrives
with our shims already installed. The manifest is then read from that repository
and its commands are executed. No prompt, no trust decision:

```
$ git clone hostile victim && cd victim
  hooks present after clone: commit-msg pre-commit pre-push prepare-commit-msg
$ git commit -m "feat: an innocent commit"
  >>> arbitrary code from the cloned repo <<<
```

Cloning a repository and committing to it is not an act of trust that anyone
performs deliberately. Reviewing a diff before running it is; nothing here asks
for that.

`pre-commit` has the same property. That is not a defence — it is a decade-old
known quantity with an ecosystem that has argued about it in public, and ours is
undocumented. `docs/custom-checks.md` presents externals purely as a
convenience. §2 of this document then proposes `stage_fixed`, which upgrades the
primitive from *run a command* to *run a command that rewrites my files and
stages the result*, and that must not ship into an untrusted manifest.

### The design

`direnv` is the precedent worth copying, because it solved exactly this: a
committed file that executes, in a directory you may have just cloned.

A manifest is **inert until trusted**. Trust records the path and a hash of the
file, so any edit re-arms it:

```
$ githooks trust          # shows the manifest, records its hash
$ githooks trust --show   # what is trusted here, and what changed since
```

Recorded per repository in git config (`githooks.trusted.<hash>`), so it is
local, never committed, and visible to the fleet dashboard like every other
policy we already surface.

An untrusted manifest is not silently skipped — that is the failure mode this
whole codebase is arranged against. It reports as `Unavailable` with a reason,
which is a shape we already have:

```
⚠ .githooks.conf declares 2 checks and is not trusted here — `githooks trust`
⚠ 2 check(s) could not run: shellcheck, smoke
```

### What this does not fix

A built-in check still runs tool binaries the repository can influence:
`resolve_tool` prefers `<root>/node_modules/.bin/<tool>`, so a hostile
`node_modules` is executed by prettier or eslint without any manifest at all.
That is inherent to running a repository's own toolchain and is the same
exposure `npm install` already carries — but it means trust for manifests is a
floor, not a ceiling, and the README should say so rather than implying the
manifest was the only door.

### Ordering

This lands **before** `stage_fixed`, and arguably before anything else here. It
is the only item on the list that is a security property rather than a
correctness or ergonomics one.

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

### pre-push has the same bug, from the other end

`pre-push` has no index at all, so this looks like a pre-commit problem. It is
not. `rust_tools::test` and `run_tests::run` compute the changed file set from
the **pushed refs** — correct, that is what is being pushed — and then run the
suite with `current_dir(dir)`, i.e. **against the working tree**:

```rust
let changed = crate::pushrefs::changed_files(refs);   // what you are pushing
let roots = cargo_roots(&root, changed.iter()…);      // where to run
each_root(&roots, None, &["test", …])                 // runs in the WORKING TREE
```

So the suite can pass on an uncommitted fix, or fail on an uncommitted
experiment, and in neither case has it tested the commits being pushed.

The fix is not the same one. Stashing is wrong here: a push is not a staging
operation, and the honest question is "does the pushed tree pass", which means
running against the pushed commit — a worktree or `git archive` of the tip
rather than the developer's tree. That is more expensive and wants its own
decision, which is why it is named here and scheduled separately rather than
folded into the stash work.

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
- **`Drop` does not run on a signal, and that is the likely case.** Ctrl-C
  during a slow pre-commit — eslint over a large tree, a cold `cargo fmt` — kills
  the process without unwinding, and the stash is orphaned. There is no signal
  handling anywhere in this codebase today. Against the paragraph above, an
  interrupt is the most probable route to losing work, not the least, so
  `StagedOnly` needs a `SIGINT`/`SIGTERM` handler that restores before exiting.
- **A recovery path for when even that fails**: `githooks restore` re-applies the
  stash this tool took. Belt and braces, because the handler can itself be
  interrupted.
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

### Decision

`cargo fmt` joins staged-only mode. Its scope is a crate, not a file list, but
that is exactly why the stage-level guard is the right fix: once unstaged
changes are stashed, `cargo fmt --check` sees staged content at the normal crate
paths and still resolves the manifest, edition and rustfmt config the same way
it does today. The misleading comment in `rust_tools.rs` should be deleted when
this lands; the trade-off was an implementation gap, not an inherent cargo
constraint.

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

**This depends on §0 and §1, and must not ship before either.** An untrusted
manifest that can rewrite files and stage the result is a worse primitive than
one that can only run a command, so the trust model is a hard precondition, not
an ordering preference.

On §1: Without the stash, "re-stage
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

## What this does not solve

Named because an adoption review asks these first, and an honest "no" is worth
more than silence.

**Tool versions are not pinned.** §1 fixes *which content* is checked and leaves
*which tool* open. `resolve_tool` prefers `<root>/node_modules/.bin/<tool>` and
falls back to `PATH`, so two developers and CI can run three prettier versions
and disagree about the same commit. `pre-commit` solves this with `rev` pinning
per hook repository, which is dismissed above as "distribution to strangers" —
that is its mechanism, not its value. Its value is determinism, and we do not
have that. Fixing content fidelity while leaving tool fidelity open is half a
reproducibility story, and the half we have is the less visible one.

**Nothing here enforces anything.** Hooks are advisory by construction:
`--no-verify` and `hook.skip` are each one command away, deliberately. So the
question "what stops an unformatted commit reaching the default branch" has no
answer in this document, and `--all-files` naming CI parity as a use case is not
one. That wants a documented exit-code contract and machine-readable output —
SARIF would let the same checks feed code scanning, JUnit would let them feed a
test report — and `githooks run --all-files` is where it belongs.

**No DCO / `Signed-off-by` check.** `commit-msg` enforces a gitmoji prefix and
length rules, which are house style. Any project that requires a Developer
Certificate of Origin needs a different check, and today it would have to be an
external — which lands it squarely in §0. It is a good candidate for a built-in
precisely because it is a policy many organisations cannot adopt the tool
without.

**musl is untested.** CI covers ubuntu, macOS and Windows. A glibc-dynamic
binary does not start in the Alpine containers a lot of pipelines use. Probably
a one-line target addition; worth knowing before somebody finds out from a
pipeline rather than from here.

## Order

**PR 0 — the trust model (§0).** First, because it is the only security property
on the list and because `stage_fixed` must not ship into an untrusted manifest.

**PR 1 — `githooks run [--all-files]`.** Small, useful immediately, no risk, and
it gives the later work a way to be exercised over a whole repository.

**PR 2 — `not_during` git-state conditions.** Generalises the `CHERRY_PICK_HEAD`
special case and closes the pre-push gap. Independently useful, and §1 needs its
predicate.

**PR 3 — index fidelity.** The correctness gap. Alone, no fixing.

**PR 4 — `stage_fixed`.** Only after 3 is proven in the fleet for a while: this
is the first feature that would write to someone's index, and it should not be
the change that also introduces the stash.

**PR 5 — pre-push runs against the pushed commits**, not the working tree. Its
own decision: a worktree or `git archive` of the tip is more expensive than
anything else here, and the cost is the whole question.

Shebang detection is unscheduled. So is everything under *What this does not
solve*, which is a list of known gaps rather than a backlog.

## Decisions

1. `--all-files` implies no stash. There is no staged/unstaged distinction to
   protect when the input set is `git ls-files`, so taking a stash would be
   surprising extra mutation with no correctness upside. If a future explicit
   `--no-stash` flag exists for diagnostics, `githooks run --all-files
   --no-stash` should be accepted as redundant rather than rejected.

   **Corollary, stated because it is the inverse of §1**: on a dirty tree,
   `--all-files` reports on content that is not committed and may never be. That
   is correct — the question it answers is "does my working tree pass", not
   "would my commit pass" — but §1 spends a page arguing that judging unstaged
   content is a bug, and a reader who meets this without warning is entitled to
   think one of the two is wrong. They are different questions; the mode's help
   text should say which one it answers.
2. `Outcome::Fixed` is invalid in `pre-push`. A pre-push hook must not modify
   the worktree or index: silently proceeding after a write would make the
   pushed commit differ from the tree the developer is now looking at.

   **Refused where every other bad declaration is refused**, rather than at push
   time. A `pre-push` line declaring a fix is a `ParseError`, alongside
   `NameTaken` and `Duplicate` — reported on every commit, named, located, and
   visible in the dashboard's `DECL` column. A "hook contract violation" raised
   at push time would be the same fact discovered later, by fewer people, in the
   one place where blocking is most expensive.

   A built-in cannot express it at all: `Fix::Rewrite` is reachable only from a
   `Stage::PreCommit` declaration, which the compiler enforces.
3. The **stash** applies to the `pre-commit` check stage only. `commit-msg`
   reads and rewrites the message file Git passes as `$1`; `prepare-commit-msg`
   appends to that same message file based on the branch name and commit
   source. Neither hook selects paths from the index or asks tools to read
   repository files, so wrapping them in `StagedOnly` would add stash risk
   without fixing a real fidelity problem.

   **`pre-push` is excluded from the stash and NOT from the problem.** It has
   the same bug by a different route (§1) — the pushed refs choose the files and
   the working tree supplies the content. Stashing is the wrong instrument
   there, so it gets its own item rather than an exemption. Saying "pre-commit
   only" and stopping is precisely the move §3 criticises: pre-push has no
   cherry-pick guard today because the zsh version had none, and nobody wrote
   down that it was a choice.
