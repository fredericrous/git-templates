# Hook architecture: one `Check` trait

Status: **specification**. Nothing here is built.

## What is wrong today

**Four tables keyed by check name**, kept in step by reconciliation tests:

| table | where | holds |
|---|---|---|
| `REGISTRY` | `githooks-runtime/registry.rs` | name → fn pointer |
| `PRE_COMMIT_CHECKS` | same file | order, pre-commit |
| `PRE_PUSH_CHECKS` | same file | order, pre-push |
| `LANGUAGES` | `githooks-fleet/checks.rs` | name → language scope |

Adding a check means editing three of them and writing a module. The tests that
keep them aligned are good tests, but they exist to police a shape that should
not be splittable in the first place.

**Three entry-point signatures and eight differently-named entry points**,
reconciled by closures in the registry. The signatures are
`(&[OsString])`, `(&str, &[OsString])` and `(&[PushRef])`; the eight functions
not called `run` are `ruff`, `pyright`, `argo_lint`, `kube_linter`,
`kubeconform`, `fmt`, `clippy` and `test`.

Three shapes is not itself scandalous — the closures adapt them fine. It matters
because a uniform signature is what lets a check be a value rather than a
special case, and that is what makes an external check indistinguishable from a
built-in to the dispatcher.

**Two models of "does this apply here".** Each check scopes itself internally
from staged files and the nearest manifest; the dashboard *separately* infers
applicability from root manifests, and `checks.rs` documents its own answer as
an approximation. Two implementations of one question, one of which admits it
is guessing.

**No extension point at all.** File-discovered sub-hooks were removed when
checks moved in-process (they had two users in 96 repos). A third party now has
no way to add a check without recompiling the binary.

**Severity is implicit in a return value.** Fifteen sites warn and then return
0. Those collapse two different situations, which is the finding that most
changes this design — see below.

## The pattern

One trait, two implementors. Strategy, with the metadata attached to the
strategy rather than kept in a parallel table.

```rust
pub trait Check {
    fn name(&self) -> &str;
    fn stage(&self) -> Stage;          // PreCommit | PrePush
    fn scope(&self) -> Scope;          // declarative; see below
    fn severity(&self) -> Severity;    // Block | Warn
    fn run(&self, ctx: &Ctx) -> Outcome;
}
```

- **`Builtin`** wraps a fn pointer. One `const` descriptor per check carries
  name, stage, scope and severity beside the function.
- **`External`** runs a command declared in a committed manifest.

The dispatcher holds `Vec<Box<dyn Check>>` — built-ins in declared order, then
externals — and stops caring which is which.

What this removes: four tables become one declaration per check, and the
reconciliation tests become **unnecessary rather than merely passing**. That is
the win. Three signatures and eight entry-point names become one shape. `scope()` becomes authoritative, so the
dashboard asks the check instead of guessing, and the approximation caveat can
be deleted rather than documented.

## `Outcome` distinguishes three things a check can mean

```rust
pub enum Outcome {
    Passed,
    Failed { detail: String },       // ran, found a problem
    Warned { detail: String },       // ran, found something non-blocking
    Unavailable { reason: String },  // COULD NOT RUN
}
```

`Unavailable` is the important addition. Today `ruff config found but no
ruff/uvx binary` prints a warning and returns 0, which is indistinguishable
from ruff running clean — to the dispatcher, and to the dashboard. A repo where
a check has silently never executed reads as a repo where it passes.

That is the same failure that `hook.skip` had before the dispatcher announced
skipped checks, and it cost three PRs to notice there. Modelling it means the
dashboard can show *ran clean* separately from *never ran*, which is the
difference between a green fleet and an unverified one.

## `Severity` is declared, and choosable

`Block` fails the stage; `Warn` reports and continues. It lives on the check, so
a built-in and an external are governed the same way.

Two consequences worth stating:

**Fail-fast applies only to `Block`.** `pre-push` stops at the first failure
because later steps are expensive and their preconditions are gone. A `Warn`
check that finds something must not stop the chain — it has not invalidated
anything.

**A severity override is a better escape hatch than `hook.skip`.**
`git config githooks.severity.<check> warn` downgrades a check instead of
disabling it. `hook.skip` is all-or-nothing and, as measured, invisible enough
that `hook.skip = e` silently disables everything. A downgrade keeps the signal
and removes only the block, which is what people usually want when they reach
for `--no-verify`. I would ship this alongside, and expect it to become the
common case.

## Scope, declared rather than reimplemented

Scoping is a **conjunction**, not a choice between alternatives:

```
ruff       .py/.pyi     AND  ruff.toml | .ruff.toml | pyproject [tool.ruff]
yamllint   .yaml/.yml   AND  .yamllint.yaml | .yamllint.yml | .yamllint
prettier   js-ish       AND  .prettierrc | .prettierrc.json | …
clippy     .rs          AND  Cargo.toml
```

So it is a struct, not an enum:

```rust
pub struct Scope {
    /// Extensions that trigger it. Empty = any change.
    files: &'static [&'static str],
    /// Config paths that opt the repo in. Empty = always on.
    opt_in: &'static [&'static str],
}
```

An earlier draft made these alternatives — `StagedFiles(..)` **or**
`Manifest(..)` — plus a `Custom` escape hatch for anything that fitted neither.
That was wrong twice over. Neither variant expresses "both", so every check with
an opt-in config would have fallen into `Custom`; an escape hatch that absorbs
most of the set leaves the dashboard knowing nothing, which is precisely the
guessing this trait exists to remove.

All twenty checks fit the struct. `merge-conflict` is `files: [], opt_in: []`.
`package-lock` is `files: [], opt_in: ["package.json"]`. `kube-linter` — one of
the cases `Custom` was invented for — reads repo-root `.kube-linter*.yaml`,
which is just an `opt_in` entry.

### Coarse declaration, precise execution

`rust_tools` resolves the NEAREST ancestor `Cargo.toml`, which no static
declaration captures. Its `Scope` therefore says `opt_in: ["Cargo.toml"]`,
meaning "somewhere in this repo" — coarser than what the check enforces when it
runs, and deliberately so.

That is safe because the two readers ask different questions. The dispatcher
asks "does this apply to the staged files of this commit" and the check answers
precisely, as it does today. The dashboard asks "would this ever fire here", for
which the coarse answer is correct. One declaration, evaluated against staged
files in one case and tracked files in the other.

Over-approximating is also the safe direction: showing a check as applicable
when it happens not to fire for a given commit is a small inaccuracy, while
`Custom` meant the dashboard could not answer at all.

## External checks

A third party cannot add a Rust module without rebuilding the binary, so
extension means declared commands.

The manifest is **committed at the repo root**, which is the point.
`.git/hooks` is not committed, so a team could never share a custom hook — a
worse flaw than the lexicographic ordering usually cited against the old
filename-prefix mechanism.

```
# .githooks.conf — stage  name        scope     severity  command
pre-commit        shellcheck  *.sh      block     scripts/lint-shell.sh
pre-push          smoke       *         warn      make smoke
```

Whitespace-delimited, order of file, ~20 lines of std parsing.

**On the format**: this is a judgement, not a constraint. The dependency guard
(`scripts/check-no-deps.sh`) is a strong default about the commit path's supply
chain, not a prohibition — see its comment. TOML would be nicer to write and
costs a dependency tree running on every commit in 96 repos. For four fields I
take the twenty lines; for a genuinely rich format the trade is worth reopening.

Externals run **after** built-ins, and cannot be reordered ahead of them: a
third-party command should not be able to delay `branch-protect`.

## Developer experience

- `githooks list` — every check, stage, scope, severity, and whether it would
  run *here*.
- `githooks explain <check>` — why it did or did not fire, in this repo, now.
  The answer to "why didn't prettier run" is currently a code-reading exercise.
- Adding a built-in: one module plus one descriptor; the compiler names what is
  missing.
- Adding an external: edit a committed file, no rebuild.
- `githooks-fleet` gains third-party checks in its views, which it cannot see
  today at all.

## Migration

**PR 1 — the trait, no behaviour change.** All 19 modules and the dispatcher.
High mechanical risk, no user-visible payoff, so it lands alone, proved inert
by a differential over hook output rather than by the test suite (tests will
legitimately change shape).

**PR 2 — `Outcome` and `Severity`.** Convert the 15 warn-and-return-0 sites to
`Warned` or `Unavailable` deliberately, one at a time, deciding which each
actually is. Add the config override. Dashboard shows *never ran* apart from
*ran clean*.

**PR 3 — external checks.** Manifest, parser, `External`, and the fleet views.

## Open decisions

1. ~~**Does `Custom` scope survive?**~~ RESOLVED: no, and the question was
   better than it looked. Checking what the checks actually key on showed the
   `Scope` enum modelled alternatives where the truth is a conjunction, so
   `Custom` would have swallowed most of the set rather than the one case it was
   written for. `Scope` is a struct now and every check fits it.
2. **Can an external check be `Block` at all?** Decided: yes, severity is on the
   trait and the author chooses. Worth revisiting if a repo ever ships a hostile
   or flaky one.
3. **Does `githooks list` belong in the hook binary or the fleet tool?** The
   fleet tool has the nicer output; the hook binary is what is installed
   everywhere.
