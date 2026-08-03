# Changelog

What an upgrader gets, in sentences. Each [GitHub
release](https://github.com/fredericrous/amont/releases) carries the
mechanical pull-request list too, generated; this file is the part a human
wrote, and the release workflow refuses to tag a version whose section is
missing here.

## Unreleased

- The README opens with the argument instead of an essay, states outright
  that the binary makes no network calls, and finally *shows* the fleet
  dashboard — `docs/assets/fleet-demo.sh` rebuilds the recording against a
  synthetic fleet, real binaries throughout.
- A test now ties the "twenty checks" prose on every user-facing page to
  `registry::CHECKS`, so the twenty-first check cannot ship with the pages
  quietly understating the count.
- Questions and ideas have a home: GitHub Discussions is on, and the issue
  templates point there.

## v1.0.2 — 2026-08-03

These releases shipped under the project's original name, githooks; the
entries keep it, because they describe what was actually released.

- `cargo install githooks` and the Homebrew tap
  (`brew install fredericrous/tap/githooks`) are live and documented.
- `githooks install` no longer copies a binary a package manager owns. A
  brew-, cargo- or distro-installed binary is baked where `PATH` exposes it
  and copied nowhere, so `brew upgrade` now reaches every repository instead
  of refreshing one file while the shims stay pointed at a frozen copy. A
  build directory on `PATH` is still copied — `cargo clean` makes it the one
  path guaranteed not to be there tomorrow.
- `githooks install` now warns when an unbaked-template setup
  (`init.templateDir` pointing at a checkout) is combined with a custom
  `$GITHOOKS_BIN_DIR`: the shims would look in `~/.local/bin` and the binary
  went elsewhere. Install names both paths and the two ways out.
- The release workflow no longer polls the crates.io index that cargo
  already waits on — the rate-limited poll could fail a publish that had
  succeeded.

## v1.0.1 — 2026-08-03

- Publishing to crates.io happens from the tag, in CI, in dependency order,
  with the credential held by the repository.
- Windows has a first-class install path: `install.ps1`, exercised in CI on
  a real Windows runner against a real published release.
- The documentation became a published book, and the repository grew its
  community files: issue templates, a PR template, a code of conduct, and a
  security policy.

## v1.0.0 — 2026-08-03

The first Rust release, ending the zsh era.

- Twenty built-in checks across four git hooks, in one std-only binary —
  commit-message conventions, merge-conflict markers, per-language linters
  and formatters, branch rules, test gates — each inert until the repository
  carries the files its tool keys on.
- The trust model: a repository declares its own checks in a committed
  `.githooks.conf`, and a clone's declarations are inert until reviewed and
  accepted with `githooks trust`, keyed on content rather than path.
- `githooks-fleet`: bulk install, fleet report, fix planning with a
  dry-run/`--apply` split, and the TUI dashboard.
- `hook.skip` and `githooks.severity` with exact matching — and every
  skipped check announced on every commit.
- An installer that verifies its download against published checksums, and
  an uninstall that removes exactly the four shims it wrote.
- v1.0.0 followed a full security review; every finding landed with a
  committed reproduction.
