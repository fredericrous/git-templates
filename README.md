# Git Templates with hooks

[![CI](https://github.com/fredericrous/githooks/actions/workflows/ci.yaml/badge.svg)](https://github.com/fredericrous/githooks/actions/workflows/ci.yaml)

Git Starter Template with opinionated hooks to help you create beautiful commits with high quality standards..and emojis ✨

<img src="https://user-images.githubusercontent.com/702227/125003867-1b012f00-e050-11eb-8641-748ef806c639.png" width="800">

*If there is an issue with a hook, please open an issue and consult the section [Opt Out](https://github.com/fredericrous/githooks/wiki/Opt-Out) for a workaround.*

## The workflow

First, `commit`. A nice template [message](https://github.com/fredericrous/githooks/blob/main/message) appears to help you write a meaningful commit description that passes the requirements.
The message saved, [validators](https://github.com/fredericrous/githooks/wiki/Hooks-implemented) run in parallel. If there is an issue, the commit is aborted
Ready to push? once `git push` is started, the tests runs for the module you updated, branch name is checked. The branch is pushed. Bravo

The wiki explore in details [this workflow](https://github.com/fredericrous/githooks/wiki/Coding-Flow)

The wiki also lists all the [implemented hooks](https://github.com/fredericrous/githooks/wiki/Hooks-implemented)

## Setup

Clone the repository to a convenient place:

```sh
mkdir -p ~/.config/git
cd ~/.config/git
git clone https://github.com/fredericrous/githooks.git
```

The clone creates `~/.config/git/git-templates/`, so there is nothing named
`templates/hooks/` in the directory you are standing in. This snippet used to
end with `chmod +x templates/hooks/*`, which matched nothing — and in `fish`, a
glob with no matches is a hard error rather than a silent no-op, so the line
either did nothing or aborted the setup depending on your shell. It is gone
rather than corrected: `githooks install` (below) sets the mode on the shims it
writes, so nobody needs to.

Setup your gitconfig

```sh
git config --global commit.template ~/.config/git/git-templates/message
```

### Two ways to turn hooks on

**Per repository** — the default. Nothing runs anywhere you did not ask:

```sh
cd <your-repo> && githooks install
githooks-fleet install --root ~/Developer   # or in bulk
```

`githooks install --force` replaces a hook the installer would otherwise refuse:
one that is present but carries no marker of ours, or one that is a symlink
(where writing normally would rewrite whatever it points at). Without the flag
install names every such file and writes none of them, because a hook somebody
else put there is somebody else's; `--force` is how you say it is yours to
replace, and the output then names what it took rather than just counting what
it wrote.

Two things `--force` does **not** override, deliberately. A **tracked** file is
never written — that is source belonging to a checkout, and it is the guard that
was got wrong twice. And a path that is a directory or a device is refused
whatever you pass, because that is not "a hook that is there", it is a sign
something else is going on; refusing costs one `rm`.

Neither of these deletes a hook it did not write. A `pre-commit-*` or
`pre-push-*` file in `.git/hooks` without our marker is reported and left
exactly where it is; `githooks-fleet --remove-unrecognized` opts into removing
them, and is spelled that way rather than `--remove-stale` because "stale" means
our own retired shims, which are a different thing entirely.

And off again — which removes our shims and nothing else. A hook you wrote
yourself is left alone and named, whatever it is: a hook it cannot even read is
named too, rather than passed over in silence. `hook.skip` and
`githooks.severity` are never touched, because those are your statements about
your repository:

```sh
githooks uninstall              # add --binary to take ~/.local/bin/githooks too
githooks-fleet uninstall --root ~/Developer
```

`githooks uninstall` also takes its shims back out of the template directory
below, and — if `init.templateDir` is still set — says so loudly with the
command to unset it. Without that, an uninstall you believed had finished left
every future `git clone` re-installing the hooks. A template directory that is
itself a checkout of this repository is never deleted from; those files are
tracked source and belong to the checkout.

**Everywhere, forever** — an opt-in, and a real one:

```sh
git config --global init.templateDir ~/.config/git/git-templates/templates
```

Git copies that directory into `.git/hooks` on every `init` **and every clone**,
so from then on every repository you clone runs these hooks without being asked
again. That is the convenience, and it is worth having: you never forget to
install, and `githooks-fleet` never shows you an uncovered repo.

It is also a standing grant, so it is worth stating what you granted. A cloned
repository can declare its own checks in `.githooks.conf`, and with this key set
those run on your first commit in it — a repository you may have cloned only to
read. If you set this, **trust the manifest deliberately** rather than relying on
installation to be the moment you decided:

```sh
githooks trust          # show what this repo declares, and accept it
githooks trust --show   # what is trusted here
```

Full reasoning in
[docs/index-fidelity-and-run-modes.md](docs/index-fidelity-and-run-modes.md) §0.

Copy the hooks to existing repositories

```sh
cd <folder-of-your-repo>
git init
```

## Update

Update the local clone to the latest version

```sh
cd ~/.config/git/git-templates/templates
git pull
```

Update the target repository

```sh
cd <your-repo> && githooks install        # re-bake the shims here
githooks-fleet fix --root ~/Developer     # or see what the whole fleet needs
githooks-fleet fix --apply --root ~/Developer
```

Ordinary binary updates need none of this: every shim points at the one binary,
so `make install` reaches all 96 repositories at once. Re-installing is only
needed when the shim SET changes — a hook added, removed or renamed.

This used to say `rm $(git rev-parse --git-dir)/hooks/*` followed by `git init`,
and that is exactly the thing this project argues against two paragraphs
earlier. The glob deletes every hook in the directory, including ones other
tools installed and ones you wrote yourself, to update four files that belong to
us. `githooks uninstall` exists so that removing our hooks never means removing
yours; a `rm *` in the README undid that promise in one line.

## Custom checks

A repository can declare checks of its own in a committed `.githooks.conf`:

```
# stage       name        scope   severity  command
pre-commit    shellcheck  *.sh    block     scripts/lint-shell.sh
pre-push      smoke       *       warn      make smoke
```

They run alongside the built-ins and obey the same `hook.skip` and
`githooks.severity.<key>` controls, addressed the same three ways: by full id
(`pre-commit-shellcheck`), by trigger (`pre-commit`), or by short name
(`shellcheck`). `githooks list` shows what would run here.
Full reference: [docs/custom-checks.md](docs/custom-checks.md).

## Naming a check, to turn it off or down

Every check has an id, `<trigger>-<name>` — `pre-commit-clippy`. Three things
name it, and both config surfaces read all three the same way:

```sh
git config --add hook.skip pre-commit-clippy   # that one check
git config --add hook.skip clippy              # that check, on either trigger
git config --add hook.skip pre-commit          # every pre-commit check
```

`githooks.severity.<key>` takes the same three:

```sh
git config githooks.severity.clippy warn       # runs, reports, does not block
git config githooks.severity.pre-commit warn   # the whole trigger
```

Where several keys reach one check the most specific wins — full id, then short
name, then trigger — so you can downgrade a trigger and exempt one check from it.
Nothing matches by substring: `hook.skip e` reaches nothing at all, and skipping
`lint-js` leaves `lint-json-yaml` alone.

A skipped check is announced on every commit, so a config line nobody remembers
writing cannot go on silently disabling things. `githooks list` shows the current
state of a repo; `githooks-fleet` shows it across all of them, with `TRIGGER` as
its own column.

## What a push actually tests

By default `pre-push` runs your suite against the **working tree**, and now says
so. That is fast and usually what you want, but it is not what you are pushing:
an uncommitted fix makes a broken commit look green.

```sh
git config githooks.testPushedTree true
```

turns on the accurate answer — the suite runs in a throwaway checkout of the
commits being pushed, and your tree is not touched. It costs a second checkout
and a build that cannot reuse your `target/` cache, which is why it is opt-in.

## Running the checks yourself

```sh
githooks run                    # would my commit pass? (the staged set)
githooks run --all-files        # does my working tree pass? (git ls-files)
githooks run pre-commit-prettier
githooks list                   # what would run here, and why not
```

The two questions are different on purpose. `--all-files` on a dirty tree
reports on content that is not committed and may never be — which is what you
want when adopting a check into an existing repository, where `git add .` is not
an acceptable way to measure the mess.

## For coding agents

`githooks list --json` is the same answer as `githooks list`, machine-readable:
every check's declared and effective severity (accounting for a
`githooks.severity.*` override), whether it fires here and why not, and its
command if it is a declared external. `--stage` filters to one trigger;
`--pushed` scopes to what your *next push* would actually carry — `@{u}..HEAD`
— rather than the whole tracked tree:

```sh
githooks list --json
githooks list --json --stage pre-push
githooks list --json --stage pre-push --pushed
```

`githooks agents-md` writes a short, self-verifying pointer to the above into
`AGENTS.md`, scoped to a `<!-- githooks:start -->` / `<!-- githooks:end -->`
block — it only ever touches that span, so the rest of the file stays yours:

```sh
githooks agents-md          # write it, or bring it up to date
githooks agents-md --check  # exit non-zero if it has drifted (missing is fine — opt-in)
```

`githooks install` offers to add this block interactively, the same way it
offers `githooks trust`. Across a fleet, `githooks-fleet` reports each repo's
state as its own `AGENTS` column, and `fix --apply --agents-md` (or
`install --agents-md`) rolls the block out — or repairs drift — everywhere at
once; like every other fleet write, it is opt-in per invocation, never bundled
into a plain `--apply`.

Design notes live in `docs/`, and each says at the top what of it ships:
[hook-architecture.md](docs/hook-architecture.md) (the `Check` trait),
[index-fidelity-and-run-modes.md](docs/index-fidelity-and-run-modes.md) (trust,
staged-only checking and run modes — what `pre-commit`, `lefthook` and `husky`
were worth taking from, and what was refused),
[fleet-dashboard.md](docs/fleet-dashboard.md),
[hook-skip-management.md](docs/hook-skip-management.md) and
[rust-migration.md](docs/rust-migration.md) (history, kept for its reasoning).

## Windows

Everything works, with one setup difference: there is no symlink.

Install with the binary itself — Git for Windows ships `bash` and coreutils but
not `make`:

```sh
cargo build --release
./target/release/githooks install
```

That is the same command `make install` runs on every platform.

On macOS/Linux `~/.config/git/git-templates` is usually a symlink to the
checkout, so `init.templateDir` can point at a stable XDG path. Windows does not
create symlinks without Developer Mode or elevation, so point git straight at
the checkout instead:

```sh
git config --global init.templateDir 'C:/path/to/git-templates/templates'
```

Nothing else changes. The shims never need the symlink: they resolve the binary
at runtime, trying `$GIT_HOOKS_BIN`, the baked path, `~/.local/bin/githooks` and
`~/.local/bin/githooks.exe`, then `PATH`. The installer detects the `.exe`
suffix on its own.

## Requirements

- **Git 2.31+**

2.31, not the 2.22 this said. `git rev-parse --path-format=absolute` landed in
2.31, and three places depend on it: `install.rs` resolving the hooks directory,
`hooks/common.rs` finding the git common dir, and `hooks/python_tools.rs`
locating a linked worktree's main `.venv`. On an older git those return nothing
rather than failing loudly, which is the worst shape for a version floor — the
tool appears to work and quietly resolves the wrong paths.

The hooks are a single Rust binary with no runtime dependencies; each check
brings its own tool requirement only where you have opted into that check (a
repo with no `ruff.toml` never needs ruff). ZSH, NodeJS and ripgrep were
requirements of the shell implementation and are no longer needed.

## Wiki

- [Coding Flow](https://github.com/fredericrous/githooks/wiki/Coding-Flow) - an explanation of where the hooks fit in your git "flow"
- [Commit Prefix](https://github.com/fredericrous/githooks/wiki/Commit-Prefix) - list of prefix your commit summaries should contain
- [Hooks Implemented](https://github.com/fredericrous/githooks/wiki/Hooks-implemented) - all the hooks that are triggered when you execute a git command
- [Ideas of hooks to implement](https://github.com/fredericrous/githooks/wiki/Ideas-of-hooks-to-implement) - a list of ideas, not a roadmap
- [Opt Out](https://github.com/fredericrous/githooks/wiki/Opt-Out) - bypass a check, a hook or uninstall it
- [Similar Projects](https://github.com/fredericrous/githooks/wiki/Similar-projects)

## Contribute

**Everything is Rust, in `crates/`.** This section used to say "if a script is
simple implement it in shell script… use javascript… a lot of devs have nodejs
installed", which is precisely the design the migration undid — and it sat
twenty lines below the sentence saying ZSH and NodeJS are no longer needed. A
new check is a module plus one registry entry, not a script.

```
crates/githooks-runtime/   the checks, registry and dispatchers. std only.
crates/githooks/           the hook binary. Runs on every commit.
crates/githooks-fleet/     the dashboard and the fleet fixer. Opt-in.
```

**The commit path stays dependency-free.** `githooks` and `githooks-runtime`
link no external crates, and `scripts/check-no-deps.sh` fails a build that
changes that. It is a strong default, not a prohibition, and the script itself
explains when reopening it is a legitimate call rather than a violation — read
it before arguing either way. `githooks-fleet` takes dependencies quite happily
(ratatui, crossterm, serde); it is installed separately and runs when asked.

The make targets:

- `make lint` — exactly what CI's `rust` job gates on: `cargo fmt --check` and
  `cargo clippy --all-targets -- -D warnings`. Note that this repo's OWN
  `pre-commit-clippy` runs a wider one (`--workspace --all-features`), so the
  hook can reject a commit CI would accept.
- `make test` — `scripts/check-no-deps.sh` plus `cargo test`. Kept clippy-free
  so the inner loop is fast. `make test RUN=<suite>` runs one.
- `make check` — `lint` then `test`. **`make` with no target runs this.**
- `make install` — builds and runs `githooks install`, which puts the binary in
  `~/.local/bin` and writes the shims into this repo and the template dir. It
  refuses to touch a template dir that is a checkout; see
  `crates/githooks-runtime/src/install.rs`.
- `make install-fleet` — the dashboard, installed separately and on purpose.
- `make propagate` — push the shim SET to every repo. Dry run; `APPLY=1` writes.
  Only needed when a hook is added, removed or renamed.
- `make deps` — the dependency guard on its own.

The toolchain is pinned in `rust-toolchain.toml`, and the floor the commit path
may require is `rust-version` in `Cargo.toml` (1.74), enforced by CI's `msrv`
job. Both are deliberate PRs to change: under `-D warnings` a new clippy release
is a breaking change, which is the whole reason the pin exists.
