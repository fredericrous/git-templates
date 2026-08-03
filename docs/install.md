# Installing and activating

Two separate acts, and keeping them separate is deliberate. **Installing the
binary** puts a program on your machine. **Activating** turns hooks on in a
repository. Nothing about the first does the second.

## 1. Get the binary

Linux and macOS:

```sh
curl -fsSL https://raw.githubusercontent.com/fredericrous/githooks/main/install/install.sh | sh
```

Windows, in PowerShell — the line above is POSIX `sh`, so on Windows it runs
only under Git Bash:

```powershell
irm https://raw.githubusercontent.com/fredericrous/githooks/main/install/install.ps1 | iex
```

The installer resolves the latest release, downloads the archive for your
platform, verifies it against the published `SHA256SUMS`, and writes the binary
to `$GITHOOKS_BIN_DIR` (default `~/.local/bin`) by atomic rename, so a
half-copied `githooks` never exists.

It refuses to guess. If the checksum does not match it exits rather than
installing; if `SHA256SUMS` is missing or there is no sha256 tool on the
machine it says loudly that the download was **not** verified rather than
staying quiet about it. Verifying what this binary is before putting it in a
position to read every staged file is the argument the project makes about its
own dependencies, applied to itself.

`~/.local/bin` is not an arbitrary default: it is candidate 3 in the shim's own
resolution order, so a binary there is found even by a shim whose baked path is
wrong. It is not "the XDG location" either — the XDG Base Directory spec
defines no binary directory at all; `~/.local/bin` is simply the convention
systemd, pipx and uv all observe. (`$XDG_CONFIG_HOME` **is** honoured, for the
template directory.)

### Installing somewhere else

`$GITHOOKS_BIN_DIR` moves the binary, and `install` bakes wherever it landed
into the shims it writes — so a custom location resolves through the baked path
and needs nothing further.

The exception is the setup where shims are deliberately left **unbaked**:
`init.templateDir` pointed at a checkout. Those shims resolve at run time from
`~/.local/bin`, which is a constant inside a POSIX `sh` file. Choose both and
they stop composing — nothing baked a path, and the one path the shims know is
not where the binary went. `githooks install` says so when it sees the
combination, and offers the two ways out: link the binary where the shims look,
or set `$GIT_HOOKS_BIN`.

`$GITHOOKS_BIN_DIR` is deliberately **not** consulted by the shim. It is an
install-time question, answered in the shell where `githooks install` ran; the
shim runs inside git's environment during a commit, where that variable is
almost never set — so honouring it there would be a knob that looks like it
works and silently does not. The runtime override already exists and is
`$GIT_HOOKS_BIN`; a second variable able to redirect which binary executes on
every commit would double that surface for nothing.

Pin a version, or install elsewhere:

```sh
GITHOOKS_VERSION=v1.0.0 GITHOOKS_BIN_DIR=~/bin \
  curl -fsSL https://raw.githubusercontent.com/fredericrous/githooks/main/install/install.sh | sh
```

### Without the installer

Download an archive and its checksum from
[Releases](https://github.com/fredericrous/githooks/releases/latest). Prebuilt
targets: `x86_64-unknown-linux-gnu`, `x86_64-unknown-linux-musl`,
`aarch64-unknown-linux-gnu`, `x86_64-apple-darwin`, `aarch64-apple-darwin`,
`x86_64-pc-windows-msvc`.

Or build it:

```sh
git clone https://github.com/fredericrous/githooks.git
cd githooks && cargo build --release
./target/release/githooks install
```

Or from [crates.io](https://crates.io/crates/githooks), which builds the same
source:

```sh
cargo install githooks
```

Or with Homebrew:

```sh
brew tap fredericrous/tap
brew trust fredericrous/tap
brew install githooks
```

The `brew trust` line is Homebrew's policy for every tap outside its own core,
not something specific to this one — a formula is code, and Homebrew now asks
you to decide about running it explicitly. Which is the same argument this tool
makes about a repository's declared checks.

### Requirements

**Git 2.31+.** `git rev-parse --path-format=absolute` landed in 2.31, and three
places depend on it: `install.rs` resolving the hooks directory,
`hooks/common.rs` finding the git common dir, and `hooks/python_tools.rs`
locating a linked worktree's main `.venv`. On an older git those return nothing
rather than failing loudly, which is the worst possible shape for a version
floor — the tool appears to work and quietly resolves the wrong paths.

Nothing else. Each check brings its own tool requirement only where you have
opted into that check: a repository with no `ruff.toml` never needs ruff.

## 2. Turn hooks on

### Per repository — the default

```sh
cd <your-repo> && githooks install
githooks list                        # what would run here, and why not
```

That writes four shims into `.git/hooks` — `pre-commit`, `pre-push`,
`commit-msg`, `prepare-commit-msg` — each of which resolves the binary at run
time and dispatches into it. Nothing runs in any repository you did not do this
in.

Across many repositories at once:

```sh
githooks-fleet install --root ~/Developer
githooks-fleet                              # report the fleet
githooks-fleet tui                          # the dashboard
```

`githooks-fleet` is installed separately and on purpose: it pulls ratatui,
crossterm and serde, and keeping the two installs apart is what stops "I wanted
the dashboard" from becoming "every commit now depends on a TUI library".

### `--force`, and what it will not do

`githooks install --force` replaces a hook the installer would otherwise
refuse:

- one that is **present but carries no marker of ours** — somebody else's hook,
  or one you wrote;
- one that is a **symlink**, where writing normally would rewrite whatever it
  points at.

Without the flag, install names every such file and writes **none** of them.
`--force` is how you say the file is yours to replace, and the output then
names what it took rather than only counting what it wrote.

Two refusals `--force` does not override:

- A **tracked** file is never written. That is source belonging to a checkout,
  and it is the guard this project got wrong twice.
- A path that is a **directory or a device** is refused whatever you pass. That
  is not "a hook that is there", it is a sign something else is going on, and
  refusing costs you one `rm`.

Neither ever deletes a hook it did not write. A `pre-commit-*` or `pre-push-*`
file in `.git/hooks` without our marker is reported and left exactly where it
is. `githooks-fleet --remove-unrecognized` opts into removing them, and is
spelled that way rather than `--remove-stale` because "stale" means our own
retired shims, which are a different thing entirely.

### Everywhere, forever — a real opt-in

```sh
mkdir -p ~/.config/git
git clone https://github.com/fredericrous/githooks.git ~/.config/git/git-templates
git config --global init.templateDir ~/.config/git/git-templates/templates
git config --global commit.template ~/.config/git/git-templates/message
```

Git copies that directory into `.git/hooks` on every `init` **and every
clone**, so from then on every repository you clone runs these hooks without
being asked again. That is the convenience, and it is worth having: you never
forget to install, and `githooks-fleet` never shows you an uncovered repo.

It is also a standing grant, and worth stating in full. A cloned repository can
declare its own checks in `.githooks.conf`. With this key set, those
declarations are present in every repository you clone — including one you
cloned only to read — and are one `githooks trust` away from running. They do
**not** run before that; see [the trust model](trust.md). But if you set this
key, trust deliberately, rather than letting installation be the moment you
decided:

```sh
githooks trust          # show what this repo declares, and accept it
githooks trust --show   # what is trusted here
```

Full reasoning: [index fidelity and run modes](index-fidelity-and-run-modes.md) §0.

## 3. Turn them off again

```sh
githooks uninstall              # this repository
githooks uninstall --binary     # …and remove the binary from ~/.local/bin
githooks-fleet uninstall --root ~/Developer
```

Uninstall removes **our four shims and nothing else**. A hook you wrote
yourself is left alone and named in the output, whatever it is — a hook it
cannot even read is named too, rather than passed over in silence. `hook.skip`
and `githooks.severity` are never touched, because those are your statements
about your repository, not ours.

It also takes the shims back out of the template directory, and — if
`init.templateDir` is still set — says so loudly, with the command to unset it.
Without that, an uninstall you believed had finished would leave every future
`git clone` re-installing the hooks. A template directory that is itself a
checkout of this repository is never deleted from; those files are tracked
source and belong to the checkout.

This is also why the documentation never tells you to run
`rm $(git rev-parse --git-dir)/hooks/*`. That glob deletes every hook in the
directory — including ones other tools installed and ones you wrote — in order
to remove four files that belong to us. `githooks uninstall` exists precisely
so that removing our hooks never means removing yours.

For bypassing a single commit, or disabling one check without uninstalling
anything, see [opting out](opting-out.md).

## 4. Keeping it up to date

Ordinary binary updates need nothing per repository: every shim points at the
one binary, so replacing the binary reaches every repo at once.

```sh
curl -fsSL https://raw.githubusercontent.com/fredericrous/githooks/main/install/install.sh | sh
```

Re-installing is only needed when the shim **set** changes — a hook added,
removed or renamed:

```sh
cd <your-repo> && githooks install        # re-bake the shims here
githooks-fleet fix --root ~/Developer     # or see what the whole fleet needs
githooks-fleet fix --apply --root ~/Developer
```

## Windows

Everything works, with one setup difference: there is no symlink.

The one-liner for this platform is PowerShell:

```powershell
irm https://raw.githubusercontent.com/fredericrous/githooks/main/install/install.ps1 | iex
```

It writes `githooks.exe` and `githooks-fleet.exe` into
`%USERPROFILE%\.local\bin`, which is where the shims look — they try both
`githooks` and `githooks.exe` there, so a binary in that directory resolves
even in a shim whose path was never baked. CI runs this script on a real
Windows runner against a real published release, because a documented install
path nobody executes is one you find out about from a bug report.

To build it yourself instead — Git for Windows ships `bash` and coreutils but
not `make`:

```sh
cargo build --release
./target/release/githooks install
```

On macOS and Linux `~/.config/git/git-templates` is usually a symlink to the
checkout, so `init.templateDir` can point at a stable XDG path. Windows does
not create symlinks without Developer Mode or elevation, so point git straight
at the checkout:

```sh
git config --global init.templateDir 'C:/path/to/githooks/templates'
```

Nothing else changes. The shims never need the symlink: they resolve the binary
at run time, trying `$GIT_HOOKS_BIN`, the baked path, `~/.local/bin/githooks`
and `~/.local/bin/githooks.exe`, then `PATH`. The installer detects the `.exe`
suffix on its own.
