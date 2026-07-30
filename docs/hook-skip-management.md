# Interactive `hook.skip` management

Status: **specification**. Nothing here is built.

Extends `docs/fleet-dashboard.md`, which listed the `s` toggle as a v2 item
without saying what it should do.

## The problem is not ergonomics

The obvious framing is that turning a check off is awkward — you must know both
the config key and the exact check name. That is true and it is the least
interesting part.

The real problem is that **`hook.skip` matches by substring**, and the
dispatcher applies it as `check_name.contains(skip_value)`:

```rust
list.iter().filter(|n| !skips.iter().any(|s| n.contains(s.as_str())))
```

Measured against the 20 checks that exist today:

| `hook.skip` value | suppresses |
|---|---|
| `pre-commit-clippy` | 1 / 20 |
| `clippy` | 1 / 20 |
| `cargo` | 2 / 20 |
| `test` | 2 / 20 |
| `lint` | 5 / 20 |
| `pre-commit` | **15 / 20** |
| `t` | **19 / 20** |
| `pre` | **20 / 20** |
| `e` | **20 / 20** |

Those numbers are **computed from today's 20 names, not a stable property** —
rename `pre-commit-lint-js` and `lint` stops suppressing five. The UI must
compute them at runtime rather than quoting this table.

None of those are adversarial. `t` is a plausible shorthand for "tests"; `pre`
is a plausible shorthand for "prettier". Either one silently disables the entire
check suite, in a config file nobody reads, with no output at commit time saying
so. A developer would discover it when something reached CI that a hook exists
to catch.

So the feature is not "let me type less". It is **make the blast radius visible
before the write happens**, and make an existing over-broad skip discoverable
after it. Everything below follows from that.

## What already shipped, and why it came first

A UX review against usage traces changed the priority. **1 of 96 repos has any
skip at all**, and its value (`run-tests-js`) was hand-written at a terminal —
the very fragment form this spec says the UI must never produce. Nobody reached
for a dashboard, because editing config takes ten seconds.

Meanwhile, at the moment a skip actually costs something — a commit running
fewer checks than the developer believes — the output was a wall of green ticks
that said nothing.

So the toggle governs a rare decision from a rarely-visited surface, while the
consequence lands on every commit. The dispatcher now announces skips:

```
  ! 15 checks skipped by hook.skip: pre-commit-argo-lint, pre-commit-ban-terms, …
```

Silent when nothing is skipped. This reaches **every** skip however it was
created, needs no dashboard, and turns `hook.skip = e` from invisible into
unmissable within one commit. It is a few lines in `dispatch::selected`.

The toggle below is still worth building, but its value is smaller than this
document originally assumed, and it should be judged as a convenience rather
than as the safety mechanism. The safety mechanism shipped.

## Non-goals

- **Not a replacement for `--no-verify`.** A one-off bypass is git's job.
  `hook.skip` is for a persistent decision, and the UI should not blur them.
- **Not a scheduler.** No expiry dates, no "skip until Friday". A skip that
  expires silently is a different surprise, not a smaller one.
- **Not fleet-wide bulk writes.** See "Scope", below — for a fleet-wide skip the
  correct mechanism is one global config entry, not 96 local ones.

## Rules

**1. The UI writes the full check name, never a fragment — but that is not
always minimal.**
`pre-commit-clippy`, not `clippy`: the fragment is a bet on every future check
name, since adding `pre-commit-clippy-nursery` would silently widen it.

The full name is still not a guarantee. **`pre-commit-lint-js` is a PREFIX of
`pre-commit-lint-json-yaml`**, so skipping the first unavoidably skips the
second, and no value expresses "this check only" under substring matching. The
UI therefore computes the radius even for a full name and requires the typed
confirmation when it exceeds one. Assuming the full name is safe is exactly the
kind of reasoning this document exists to prevent.

**2. Lead with what protection is lost, not with a count.**
"Suppresses 19 of 20" is an aggregate, and aggregates do not move people the way
a named consequence does. It also treats every check as interchangeable, which
they are not: `pre-commit-yamllint` reformats, `pre-push-branch-protect` is what
stops a push to `main`.

Name them, most consequential first; the count is a subtitle.

```
 skip pre-commit-ban-terms in Perso/homelab?

   git config --add hook.skip pre-commit-ban-terms

   suppresses 1 of 20 checks:
     pre-commit-ban-terms

   [y] write   [esc] cancel
```

**3. An over-broad EXISTING skip is reported as a finding.**
The UI cannot stop someone writing `hook.skip = e` by hand, so it must surface
it. A skip suppressing more than one check appears in the repo detail view with
its full expansion, and in the hook view as the reason a check shows `skipped`:

```
 hook.skip
   pre-commit-clippy    -> pre-commit-clippy
   t                    -> 19 checks  !  probably not intended
```

The threshold is "more than one", not a percentage. Two is already a surprise if
you meant one.

And offer the correction rather than only the diagnosis. Told "this suppresses
19 checks", the user is informed they are wrong and left there. The one skip
that exists in the fleet today, `run-tests-js`, is exactly this case: the UI
should recognise a fragment and offer `pre-push-run-tests-js` in its place.

**4. Removal is exact, and the exit code is not evidence.**

`git config --unset hook.skip <value>` takes a value-pattern that is a **regex**,
not a literal. Measured:

- `--unset hook.skip 'pre-commit-lint.js'` removes `pre-commit-lint-js` — the
  `.` is a wildcard. Any value the UI does not escape can over-match.
- When the pattern matches **more than one** value, git prints
  `warning: hook.skip has multiple values`, **removes nothing, and exits 5**.
  An earlier revision of this document said it exits 0 and called that a silent
  no-op; that was a mis-measurement — `$?` was read after a pipe, so it reported
  the last command in the pipeline rather than git. git does signal the refusal.
- `--unset-all` with the same fragment removes **both**, which is the opposite
  surprise, and duplicates are legal so an anchored pattern can match twice.

So the UI passes an anchored, escaped pattern (`^pre-commit-clippy$`) and then
**re-reads the config to confirm**. Not because the exit code lies, but because
a status code reports what the command believes it did while a re-read reports
what is true. Config is small and the read is cheap; there is no reason to
prefer the weaker evidence.

**5. Differentiate the interaction by risk; prefer undo to confirmation.**
The draft gave a 1-of-20 and a 19-of-20 skip the same modal and the same `[y]`.
Frequency data says habituation is not the danger here — you cannot habituate to
something done twice a year — but flattening still costs the one signal that
matters: when every case looks identical, the dangerous case has no way to feel
dangerous.

- **One check suppressed** — write it, no prompt, and offer `u` to undo. A skip
  is reversible by a single command, and an undo helps when the user was wrong
  rather than merely interrupting when they were right (Nielsen's third
  heuristic; the Undo Send precedent).
- **More than one** — require typing the check name, not a keypress. Chosen
  because it cannot be muscle-memoried.

The UI never refuses. Showing the damage is the intervention; refusing invites
working around the tool by hand, which is worse and unobservable.

**6. Idempotent.**
`--add` on a value already present creates a duplicate that must then be unset
twice. The UI checks first and reports "already skipped" rather than writing.

## Scope: local, or global

Two scopes, and the distinction matters more than it looks.

- **Local** (`git config --add`, writes `.git/config`) — this repo only. The
  default, because it is the reversible one and it is where a
  repo-specific decision belongs.
- **Global** (`git config --global --add`) — every repo on the machine,
  including ones cloned later.

A fleet-wide skip must use **global**, not a loop over 96 local writes. One
entry is one thing to find and one thing to undo; 96 entries are a migration in
their own right, and the tool has already learned what a half-applied sweep
costs. The UI therefore offers global explicitly rather than synthesising it,
and labels it as affecting every repository — present and future.

`configured_skips` reads via plain `git config --get-all`, so global and local
entries are already merged with no way to tell them apart at dispatch time. The
detail view must show which scope each value came from
(`git config --show-origin --get-all hook.skip`), or a developer will delete a
repo's `.git/config` line and be baffled that the check is still skipped.

## Where it lives in the UI

**Repo detail (`Enter`, then `s`)** — the check list becomes selectable, and `s`
toggles the highlighted check for that repo. This is the common case: "not in
this repo, not right now".

**Hook view (`h`, then `s`)** — offers the *global* skip for the selected check,
with the affected-repo count from the `APPLICABLE` column. This is the rarer,
larger decision, and it should feel larger.

Both go through the same preview. Neither writes on a single keypress.

## Data model

```
SkipEntry {
  value      : String       // as written in config
  scope      : Local | Global | Other(path)
  suppresses : Vec<&'static str>   // resolved against the check registry
}

SkipPlan {
  repo       : PathBuf
  scope      : Scope
  action     : Add(String) | Remove(String)
  command    : Vec<String>          // exactly what will be run
  suppresses : Vec<&'static str>    // what changes as a result
  refuse     : Option<Refusal>      // already present / not applicable / unreadable config
}
```

`suppresses` is resolved through the same substring rule the dispatcher uses.
Reimplementing the match differently is how a UI comes to claim a check is
active while the dispatcher skips it — the existing `checks::is_skipped` already
shares the rule and this must too.

## Tests

The important ones are about honesty, not mechanics:

- A one-character skip is reported as suppressing 19–20 checks, by name.
- The written value is always the full check name — falsified by writing a
  fragment and asserting the test fails.
- Adding an existing skip is refused, not duplicated.
- Removal uses an anchored pattern and is verified by re-reading config.
- A global skip is labelled as global in the detail view, with its origin.
- The resolver agrees with `dispatch::selected` for the same inputs — a property
  test over the check list is cheap here and pins the two together.

## How we would know it worked

The draft had no success measure, which makes a design shippable but not
evaluable.

- **Zero skips suppressing more than one check unintentionally.** Measurable
  from `githooks-fleet --json` at any time.
- **Time-to-discover an over-broad skip.** The shipped announcement drives this
  to one commit; before it, the honest answer was "possibly never".
- **Fragment-valued skips trend to zero.** Today: one of one.

## Rendering

Inherits the dashboard's constraints and must not quietly drop them: the preview
is legible under `NO_COLOR`, degrades below 100 and 60 columns rather than
scrolling sideways, and encodes nothing by colour alone.

## Open questions

1. ~~**Should the UI ever refuse a write?**~~ RESOLVED: no. Show the damage.
   Refusing protects against a mis-keystroke but invites working around the tool
   by hand, which is both worse and unobservable.
2. **Should `skips` in the fleet table show a count or the values?** A count is
   compact and hides an over-broad entry; the values do not fit in a column.
   Probably a count plus a marker when any entry suppresses more than one.
3. **Is `--show-origin` fast enough per repo at fleet scale?** It is one extra
   `git config` invocation across ~96 repos. Measure before adding it to the
   scan rather than the detail view.
