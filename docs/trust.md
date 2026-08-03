# The trust model

**A repository you clone cannot run code on your machine until you say it may.**

That is the whole claim. This page is how it is enforced and where it stops.

## The problem it solves

A repository can declare checks of its own in a committed
[`.githooks.conf`](custom-checks.md), and those declarations are shell
commands. Committing the file is the point — it is how a team shares a check.

The consequence is that cloning a repository and committing to it would
otherwise run commands *that repository chose*, and neither of those acts is
one anybody performs as a decision about trust. Reviewing a diff before running
it is such a decision. Nothing asked for that.

This matters most for people who set `init.templateDir`
([see install](install.md)), because for them the hooks are already present in
every repository they clone — including one cloned only to read.

## What happens instead

A manifest is **inert until trusted**. Its declared checks are listed by
`githooks list` with the reason they will not fire:

```
declared in an untrusted .githooks.conf — review it, then `githooks trust`
```

```sh
githooks trust          # show what this repo declares, and accept it
githooks trust --show   # what is trusted here
githooks trust --revoke # forget it
```

`githooks trust` prints the declarations before asking. That is not politeness:
"trust this file" is not a question anybody can answer without seeing it, and a
prompt that does not show the file is a prompt that trains people to press `y`.

`githooks trust` outside a repository is refused rather than falling back to
`.`. Trust is recorded per repository, keyed by the root that resolves — so a
`.` fallback would let `githooks trust` in `~` read `~/.githooks.conf`, show
its declarations, and record trust against a repository that does not exist, in
a state no later `--revoke` would find.

## Consent is bound to content, not to a path

The record is a **fingerprint of the file**, stored in `--local` git config
under `githooks.trusted` — local, never committed, so a repository cannot
declare itself trusted.

Because it is keyed on content, a `git pull` that adds a command does not
inherit the consent given to the file before it. That state is reported
distinctly from "never trusted", because *somebody changed it* is a different
thing to tell a reader than *you have not looked at this yet*:

```
.githooks.conf changed since it was trusted — review it, then `githooks trust`
```

## Why `git hash-object --no-filters`

`githooks` links no external crates ([and CI enforces
that](https://github.com/fredericrous/githooks/blob/main/scripts/check-no-deps.sh)),
and the only hash in `std` is `DefaultHasher` — SipHash with a fixed key, not
collision-resistant, so a crafted manifest could be made to match a trusted
one's fingerprint. Hand-writing SHA-256 is a hundred lines nobody would review
as carefully as they should.

`git` is already a hard dependency of every path in this binary, and
`git hash-object` is the identity git itself uses for content. It is SHA-1 (or
SHA-256 in a repository configured for it) — not a guarantee against a
determined attacker with a chosen-prefix collision, but enormously better than
SipHash, it costs no dependency, and **you can reproduce it by hand** to check
what you trusted:

```sh
git hash-object --no-filters .githooks.conf
git config --local --get githooks.trusted
```

`--no-filters` is the load-bearing flag. Without it, git applies the clean
filter and eol conversion that the repository's *own committed*
`.gitattributes` asks for — so the repository would be choosing the transform
its consent is taken through, and two manifests the parser reads differently
could be given the same id. Consent is bound to the bytes that are **parsed**.

For the same reason, where a caller already holds the file's bytes, the state
is decided about *those* bytes rather than by re-opening the path. Two reads of
a file somebody is deciding about can disagree, and the decision would then be
recorded about bytes nobody was shown.

## Two windows that were closed, and how

**Between showing and answering.** `githooks install` prints the manifest, then
blocks on a keypress — sometimes for several seconds — before recording
anything. Re-hashing at that point would trust whatever is on disk *then*,
which is not necessarily what was shown. So callers fingerprint what they show
*before* asking, and pass that same value back to be verified again once the
answer is in. If the file changed in the window, nothing is trusted and the
prompt says so.

**Concealing a declaration inside the listing.** Every field in that listing is
repository-controlled text, and it is the text somebody is about to say yes to.
It is sanitised **before** the column padding is computed: a terminal escape
sequence is zero columns wide, so it would silently shift the alignment even if
it did nothing worse — and a repository that can move the rendering can hide a
line from the person consenting to it. A repository must not be able to pick
how its own consent is rendered any more than it can pick how it is hashed.

## What this does not protect against

Stated plainly, because a security boundary described only by what it stops is
a marketing claim:

- **It is not a sandbox.** Once you trust a manifest, its commands run with
  your privileges. Trust is a review gate, not containment.
- **It says nothing about the built-in checks.** Those are code in the binary
  you installed, and are governed by
  [`hook.skip` and severity](opting-out.md), not by trust.
- **It does not protect a repository you wrote the manifest in.** Your own
  `.githooks.conf` is trusted by you, once.
- **SHA-1 is the floor**, in a repository using git's default object format.
  See above for why that trade was taken.

To report something this model gets wrong, see
[SECURITY.md](https://github.com/fredericrous/githooks/blob/main/SECURITY.md).
