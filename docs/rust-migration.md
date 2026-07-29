# Migrating the hooks to a single Rust binary

Status: proposed, not started. Written 2026-07-29.

## Why

Two requirements the current design cannot meet:

**1. No unconditional runtime dependency.** `commit-msg` is node and runs on
*every commit in every repo*. The `pre-commit` / `pre-push` dispatchers are zsh.
So a pure-Python repository needs **node and zsh installed to make a commit** —
not because it uses either, but because the hook framework does. Per-hook
scoping cannot fix this: the dependency is in the entrypoint, before any scoping
runs.

**2. Windows.** 17 of 20 hooks are `#!/bin/zsh`. Git for Windows ships MSYS2
bash, not zsh, so those hooks do not execute at all. Re-shebanging is not enough
— they use zsh-only syntax throughout (23 `${0:a:h}`-style modifier expressions,
45 bare-word arrays, 5 `(N)` glob qualifiers, 2 `setopt`).

A single static binary answers both: nothing to install alongside it, and
Windows is a build target rather than a rewrite.

**What this is NOT for.** The nine linter-orchestration hooks shell out to
eslint / prettier / ruff / pyright / yamllint / kubeconform / kube-linter / argo.
Rust does not remove those, and should not: they are opt-in per repo already
(each hook no-ops unless the tool's config is present), which is exactly the
"only the tools I actually use" behaviour we want. A Python repo pulls `ruff`,
not `eslint`. That part of the design is already right.

## Scope

| group | files | lines | why it moves |
| --- | --- | --- | --- |
| entrypoints + always-on | `pre-commit`, `pre-push`, `commit-msg`, `prepare-commit-msg`, `ban-terms`, `branch-pattern`, `usual-name`, `pull-rebase`, `run-tests-js` | ~700 | runs regardless of repo language — the actual dependency |
| linter orchestration | 9 × `pre-commit-*.zsh` | ~576 | only for Windows parity; otherwise optional |
| tests | 15 × `tests/*.zsh` | ~859 | see below — these are an asset, not a cost |

## The thing that de-risks this

**The existing test suite is interface-level.** Every test invokes the hook by
file path and asserts on exit code and stdout; none reaches inside. So once a
hook file becomes a shim that `exec`s the binary, all 14 suites keep working
**unchanged** against a Rust implementation.

That means a behavioural regression net for the whole migration, on Linux and
macOS via the CI added in #12, before writing a single line of Rust test. Port a
hook, run `make test`, and the same assertions that guarded the zsh version now
guard the Rust one. Port the tests to `cargo test` later, or never.

Treat this as a hard rule: **a hook is not ported until its existing `.zsh` test
passes against the binary, unmodified.**

## The two genuinely hard parts

### 1. Shim resolution (the part that will bite)

Git requires an executable file at each hook name, so the binary needs ~4 thin
shims: `commit-msg`, `pre-commit`, `pre-push`, `prepare-commit-msg`.

```sh
#!/bin/sh
exec git-hooks pre-commit "$@"
```

The trap: **git hooks do not inherit an interactive shell's PATH.** GUI clients
(VS Code, Tower, SourceTree, JetBrains) launch git with a login-ish environment
that frequently lacks `~/.local/bin` or `~/.cargo/bin`. A shim that only does
`exec git-hooks` works in the terminal and fails silently in the GUI — the worst
possible failure shape for something standing between a person and their commit.

Resolve in this order, and bake the install path in at `make install` time:

1. `$GIT_HOOKS_BIN` (escape hatch, and how the tests point at `target/debug`)
2. the absolute path written into the shim by `make install`
3. `command -v git-hooks` (PATH, last resort)
4. **fail loudly** — never silently skip a check

Windows: the `#!/bin/sh` shim runs under Git for Windows' bundled sh, which is
always present, and MSYS resolves `git-hooks` to `git-hooks.exe`. Keep the shim
rather than installing a bare `.exe` as the hook file.

### 2. Distribution

This is the real new cost, and it is permanent machinery we do not have today.

- Build 5 targets in CI: `aarch64-apple-darwin`, `x86_64-apple-darwin`,
  `x86_64-unknown-linux-musl` (static — no glibc-version coupling),
  `aarch64-unknown-linux-musl`, `x86_64-pc-windows-msvc`.
- Publish on tag, with checksums. `cargo-dist` generates most of this.
- `make install` fetches the artifact for the host triple, verifies the
  checksum, installs the binary, then writes the shims with the resolved path.
- Contributor fallback: `cargo install --path .`.
- Keep a `make install-from-source` path so a machine without network (or on an
  unlisted triple) is not stuck.

**Offsetting win:** updating today means copying N files into 96 repos (done
twice this week). After this, it is replacing **one binary** — the shims change
almost never. Net, install gets *simpler*, not harder.

## Behaviour that must be preserved

Verified against the current dispatcher — easy to lose in a rewrite:

- **Sub-hooks run in PARALLEL** (`pre-commit` backgrounds each and waits). A
  serial Rust port would be a visible slowdown on every commit.
- **Failure reporting names the failing hook(s)**, not just a non-zero exit.
- **`git config --get-all hook.skip`** filters by substring match against the
  hook path. `git -c hook.skip=package-lock commit` must keep working.
- **`CHERRY_PICK_HEAD` short-circuit**: the dispatcher exits 0 during a cherry-pick.
- **`HOOKS_FORCE_GREP`** exists only to exercise the grep fallback in tests; it
  disappears with the shell hooks, along with the `rg`/`grep` split itself —
  the binary uses the `regex` crate and needs neither.

## Phases

Each phase ships and leaves the repo working. Nothing is a flag day.

**Phase 0 — scaffold, no behaviour change.**
Cargo crate, CI build job, shim mechanism, `make install` writing shims.
The Rust `pre-commit`/`pre-push` dispatchers do nothing themselves yet: they
glob and run the existing `pre-commit-*` / `pre-push-*` script files exactly as
the zsh dispatcher does, in parallel, honouring `hook.skip`.
*Done when:* all 14 zsh suites pass unchanged with shims installed.

**Phase 1 — the always-on set.** In this order (cheapest correctness first):
`branch-pattern` → `usual-name` → `prepare-commit-msg` → `pull-rebase` →
`commit-msg` → `ban-terms` → `run-tests-js`.
Each: implement, delete the script, `make test` must stay green.
*Done when:* a repo with no linter configs needs **no zsh and no node** to commit.
That is requirement 1, delivered.

`ban-terms` earns a real parser here. Its current comment/string blanker is
explicitly "not a parser" and mis-handles a regex literal containing an escaped
slash; a proper tokenizer removes that whole class of false negative.

**Phase 2 — Windows.** Add `windows-latest` to CI. Only meaningful once Phase 1
lands, because the remaining zsh hooks still cannot run there.
*Done when:* the always-on set is green on Windows in CI.

**Phase 3 — linter orchestration (optional).** Port the nine `pre-commit-*.zsh`.
Required only for full Windows parity; skip it if Windows is a "commit hygiene"
goal rather than a "run every linter" goal.
*Done when:* zsh is absent from the repo entirely.

**Phase 4 — retire the shell test suite (optional).** Port `tests/*.zsh` to
`cargo test`. Deliberately last: those tests are the migration harness, and
rewriting them early would remove the net while walking the wire.

## Rollout to existing repos

96 local repos hold copies from `git init` time. The established recipe
(compare each installed copy against every historical blob of that file, replace
only exact matches so a customised copy is never clobbered) applies unchanged to
the shims — see the propagation note in the ban-terms memory. Survey found zero
customised copies, twice.

After Phase 0 the shims are stable, so subsequent phases need **no** repo sweep:
updating the binary updates every repo at once.

## Decisions to make before starting

1. **Binary name.** `git-hooks` collides conceptually with `git hooks` (git may
   resolve `git-hooks` as a subcommand). Prefer something unambiguous.
2. **One binary or one per hook?** One, with the hook name as argv[1]. Five
   binaries would mean five downloads and five things to keep in sync.
3. **Config format.** The zsh hooks read `git config` (`hook.skip`). Keep that,
   or introduce a `.githooks.toml`? Keeping `git config` avoids inventing a
   second source of truth and preserves `git -c hook.skip=… push`.
4. **MSRV and dependency budget.** `regex` and `ignore` (ripgrep's own crates)
   cover matching and file walking. Resist more.
5. **Does Windows need the linter hooks?** Determines whether Phase 3 is
   optional or mandatory.

## Honest cost

~700 lines of shell/JS to port for Phase 1, plus a release pipeline that must be
maintained forever. Against that: the framework stops imposing node and zsh on
every commit, Windows becomes a build target, and the BSD/GNU divergence class
disappears — that class produced two of the three bugs CI found on its first two
runs (`realpath -s`, and the `\w`-vs-POSIX-class dialect split).

If Phase 1 alone lands, the stated requirement is met and Phases 2-4 remain
genuinely optional.
