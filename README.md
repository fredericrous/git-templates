# Git Templates with hooks

[![CI](https://github.com/fredericrous/git-templates/actions/workflows/ci.yaml/badge.svg)](https://github.com/fredericrous/git-templates/actions/workflows/ci.yaml)

Git Starter Template with opinionated hooks to help you create beautiful commits with high quality standards..and emojis ✨

<img src="https://user-images.githubusercontent.com/702227/125003867-1b012f00-e050-11eb-8641-748ef806c639.png" width="800">

*If there is an issue with a hook, please open an issue and consult the section [Opt Out](https://github.com/fredericrous/git-templates/wiki/Opt-Out) for a workaround.*

## The workflow

First, `commit`. A nice template [message](https://github.com/fredericrous/git-templates/blob/main/message) appears to help you write a meaningful commit description that passes the requirements.
The message saved, [validators](https://github.com/fredericrous/git-templates/wiki/Hooks-implemented) run in parallel. If there is an issue, the commit is aborted
Ready to push? once `git push` is started, the tests runs for the module you updated, branch name is checked. The branch is pushed. Bravo

The wiki explore in details [this workflow](https://github.com/fredericrous/git-templates/wiki/Coding-Flow)

The wiki also lists all the [implemented hooks](https://github.com/fredericrous/git-templates/wiki/Hooks-implemented)

## Setup

Clone the repository to a convenient place:

```sh
mkdir ~/.config/git
cd ~/.config/git
git clone https://github.com/fredericrous/git-templates.git
chmod +x templates/hooks/*
```

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

And off again — which removes our shims and nothing else. A hook you wrote
yourself is left alone and named; `hook.skip` and `githooks.severity` are never
touched, because those are your statements about your repository:

```sh
githooks uninstall              # add --binary to take ~/.local/bin/githooks too
githooks-fleet uninstall --root ~/Developer
```

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
rm $(git rev-parse --git-dir)/hooks/*
git init
```

## Custom checks

A repository can declare checks of its own in a committed `.githooks.conf`:

```
# stage       name        scope   severity  command
pre-commit    shellcheck  *.sh    block     scripts/lint-shell.sh
pre-push      smoke       *       warn      make smoke
```

They run alongside the built-ins and obey the same `hook.skip` and
`githooks.severity.<name>` controls. `githooks list` shows what would run here.
Full reference: [docs/custom-checks.md](docs/custom-checks.md).

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

Design notes live in `docs/`:
[hook-architecture.md](docs/hook-architecture.md) (the `Check` trait, shipped),
[index-fidelity-and-run-modes.md](docs/index-fidelity-and-run-modes.md) (what
`pre-commit`, `lefthook` and `husky` are worth taking from — specification),
[fleet-dashboard.md](docs/fleet-dashboard.md) and
[hook-skip-management.md](docs/hook-skip-management.md).

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

- Git 2.22+

The hooks are a single Rust binary with no runtime dependencies; each check
brings its own tool requirement only where you have opted into that check (a
repo with no `ruff.toml` never needs ruff). ZSH, NodeJS and ripgrep were
requirements of the shell implementation and are no longer needed.

## Wiki

- [Coding Flow](https://github.com/fredericrous/git-templates/wiki/Coding-Flow) - an explanation of where the hooks fit in your git "flow"
- [Commit Prefix](https://github.com/fredericrous/git-templates/wiki/Commit-Prefix) - list of prefix your commit summaries should contain
- [Hooks Implemented](https://github.com/fredericrous/git-templates/wiki/Hooks-implemented) - all the hooks that are triggered when you execute a git command
- [Ideas of hooks to implement](https://github.com/fredericrous/git-templates/wiki/Ideas-of-hooks-to-implement) - a list of ideas, not a roadmap
- [Opt Out](https://github.com/fredericrous/git-templates/wiki/Opt-Out) - bypass a check, a hook or uninstall it
- [Similar Projects](https://github.com/fredericrous/git-templates/wiki/Similar-projects)

## Contribute

Basically if a script is simple implement it in shell script. If the logic is complicated, use javascript or any proper language to implement it. Javascript is nice because nowadays a lot of devs have nodejs installed on their machine.

There's a makefile, open it, see the different tasks, basically:

- `make test` runs the tests
- `make` is an alias to `make test`
- `make install` builds and runs `githooks install`, which puts the binary in
  `~/.local/bin` and writes the shims into this repo and the template dir. It
  refuses to touch a template dir that is a checkout — see `crates/githooks-runtime/src/install.rs`

To run only one test, use `make test RUN=<part of the name of the test>`
