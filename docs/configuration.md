# Configuration

Everything is `git config`. There is no config file of ours, no `.githooksrc`,
nothing to keep in sync — the settings live where a git user already looks for
settings, and `--local`/`--global` mean what they always mean.

To bypass or disable rather than tune, see [opting out](opting-out.md).

## Naming a check

Every check has an id, `<trigger>-<name>` — `pre-commit-clippy`. **Three things
name it**, and every config surface reads all three the same way:

| key | reaches |
|---|---|
| `pre-commit-clippy` | that one check |
| `clippy` | that check, on either trigger |
| `pre-commit` | every check on that trigger |

Where several keys reach one check, **the most specific wins**: full id, then
short name, then trigger. So you can downgrade a whole trigger and then exempt
one check from that downgrade.

**Nothing matches by substring.** `hook.skip e` reaches nothing at all, and
skipping `lint-js` leaves `lint-json-yaml` alone — a skip can never silently
couple two checks whose names happen to share a prefix.

## `hook.skip` — do not run it

Multi-valued; add as many as you need.

```sh
git config --add hook.skip pre-commit-clippy   # that one check
git config --add hook.skip clippy              # on either trigger
git config --add hook.skip pre-commit          # the whole trigger
git config --unset-all hook.skip               # start over
git config --get-all hook.skip                 # what is set here
```

A skipped check is **announced on every commit**. A config line nobody
remembers writing cannot go on silently disabling things — that silence is how
a repository ends up with a check everyone believes is running.

For one commit only, without touching config:

```sh
git -c hook.skip=clippy commit -m "fix: …"
```

## `githooks.severity.<key>` — run it, but do not block

Takes the same three spellings, and keeps the signal: the check still runs and
still reports, it just stops failing the commit.

```sh
git config githooks.severity.clippy warn       # runs, reports, does not block
git config githooks.severity.pre-commit warn   # the whole trigger
git config githooks.severity.clippy block      # back to blocking
```

`warn` is usually the right first move when adopting a check into an existing
repository: you get the report immediately and pay down the backlog on your own
schedule, rather than choosing between a blocked commit and a `hook.skip` you
will forget to remove.

## `githooks.testPushedTree` — test what you are pushing

```sh
git config githooks.testPushedTree true
```

By default `pre-push` runs your suite against the **working tree**, and says
so. That is fast and usually what you want, but it is not what you are pushing:
an uncommitted fix makes a broken commit look green.

With this set, the suite runs in a throwaway checkout of the commits being
pushed, and your tree is not touched. It costs a second checkout and a build
that cannot reuse your `target/` cache, which is why it is opt-in rather than
the default.

## `githooks.trusted`

Set by `githooks trust`, read by everything that decides whether a declared
external may run. `--local` only, never committed. Do not set it by hand — see
[the trust model](trust.md).

## `commit.template`

Not ours, but worth setting: it puts the footer scaffold in front of you when
you write a commit.

```sh
git config --global commit.template ~/.config/git/git-templates/message
```

## Environment variables

| variable | effect |
|---|---|
| `GIT_HOOKS_BIN` | Absolute path to the binary a shim should use. First candidate in the shim's resolution order. |
| `GITHOOKS_BIN_DIR` | Where `githooks install` and the installer script put binaries. Default `~/.local/bin`. |
| `GITHOOKS_VERSION` | Pins the version the installer script fetches. |
| `NO_COLOR` | Honoured, as is a non-tty stdout. |

## Repository-declared checks

A repository can add checks of its own without anybody forking anything, in a
committed `.githooks.conf`. They obey every control on this page, addressed the
same three ways, and they are inert until trusted.

Full reference: [custom checks](custom-checks.md) ·
[the trust model](trust.md).

## Seeing the result

```sh
githooks list              # what would run here, and why not
githooks list --json       # the same, machine-readable
githooks-fleet             # the same, across every repository
```

`githooks list` reports the **effective** severity, after overrides — so a
check you downgraded three months ago is visible as downgraded rather than
having to be inferred from config. Across a fleet, `githooks-fleet` shows
skips and severities per repository, with `TRIGGER` as its own column.
