# Opting out

Four different things get called "turning it off". They are listed here
smallest first, because the right answer is usually the smallest one.

> **Migrated from the old GitHub wiki, and corrected.** The wiki page described
> `hook.skip` as a glob pattern — "`hook.skip=lint` would match
> `pre-commit-lint-js` and `pre-commit-lint-json-yaml`" — and told you to opt
> out by editing `exit $EXIT_CODE` out of a shell script, or by `rm`-ing
> `.git/hooks`. None of that is true of the current implementation, and two
> parts of it were actively harmful. See below.

## 1. One command: `--no-verify`

```sh
git commit --no-verify
git push --no-verify
```

Skips the whole `pre-commit` or `pre-push` stage for that one invocation.

`commit-msg` and `prepare-commit-msg` are **not** bypassable this way — that is
git's behaviour, not ours. To get a message past `commit-msg`, fix the message.

## 2. One run, one check: `-c`

```sh
git -c hook.skip=clippy commit -m "fix: …"
git -c hook.skip=clippy -c hook.skip=prettier commit -m "fix: …"
```

Nothing is written to config, so there is nothing to remember to undo.

## 3. Permanently, in this repository

```sh
git config --add hook.skip pre-commit-clippy   # that one check
git config --add hook.skip clippy              # that check, either trigger
git config --add hook.skip pre-commit          # every pre-commit check
```

**These are exact names, not globs.** `hook.skip e` reaches nothing at all, and
skipping `lint-js` leaves `lint-json-yaml` alone. The old glob behaviour
coupled checks whose names happened to share a prefix, which meant disabling
one thing silently disabled another; [skip
management](hook-skip-management.md) records why it went.

To see and undo:

```sh
git config --get-all hook.skip
git config --unset-all hook.skip
```

A skipped check is **announced on every commit**, on purpose. A `hook.skip`
line nobody remembers writing is exactly how a repository ends up with a check
everyone believes is running.

### Usually better: warn instead of skip

```sh
git config githooks.severity.clippy warn
git config githooks.severity.pre-commit warn
```

The check still runs and still reports; it just stops failing the commit. You
keep the signal, which is the thing `hook.skip` throws away. This is the right
first move when adopting a check into an existing repository with a backlog.

The old wiki's answer to this question was to edit `exit $EXIT_CODE` out of the
`pre-commit` script. That worked, and it also meant your opt-out lived in a
file the next update overwrote, invisible to anyone reading the repository.

## 4. Remove the hooks entirely

```sh
githooks uninstall              # this repository
githooks uninstall --binary     # …and the binary from ~/.local/bin
githooks-fleet uninstall --root ~/Developer
```

This removes **our four shims and nothing else**. A hook you wrote yourself is
left alone and named in the output, whatever it is; a hook it cannot even read
is named too, rather than passed over in silence. `hook.skip` and
`githooks.severity` are never touched — those are your statements about your
repository.

If `init.templateDir` is still set, uninstall says so loudly and gives you the
command to unset it. Without that, an uninstall you believed had finished would
leave every future `git clone` re-installing the hooks:

```sh
git config --global --unset init.templateDir
```

### Do not use `rm .git/hooks/*`

The old wiki said "you can disable the hooks any time by removing the files in
`$(git rev-parse --git-dir)/hooks/`". That glob deletes **every** hook in the
directory — including ones other tools installed and ones you wrote — in order
to remove four files that belong to us. `githooks uninstall` exists precisely
so that removing our hooks never means removing yours.

## A repository is asking to run its own checks

If `githooks list` shows a check with:

```
declared in an untrusted .githooks.conf — review it, then `githooks trust`
```

then that repository has declared checks and they are **already not running**.
There is nothing to opt out of. Opting *in* is `githooks trust`, after reading
the file. See [the trust model](trust.md).
