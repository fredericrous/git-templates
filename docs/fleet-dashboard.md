# `githooks fleet` — a TUI for the 96-repo hook fleet

Status: **built**. PRs #41-#47 implement v1 and v2; `scripts/propagate.sh` is
gone, replaced by `githooks-fleet fix`.

Two items from the plan are deliberately unbuilt: the `s` toggle for `hook.skip`
and the `:` command palette. Both are useful, neither is load-bearing, and a
named gap is better than a half-built one.

The skip toggle is specified separately in
[`hook-skip-management.md`](hook-skip-management.md) — it turned out to be a
safety feature rather than an ergonomic one, since `hook.skip` matches by
SUBSTRING and `hook.skip = e` silently disables all 20 checks.

## Why this exists

`scripts/propagate.sh` prints a text summary. That summary has misled its author
twice, on the same day:

- It reported **192** removals per hook name across **96** repos that hold one
  copy each. Two overlapping loop conditions were both claiming the same files.
  The number is arithmetically impossible, and it still read as plausible.
- A consistency sweep reported `0 copies / 0 distinct` for every hook. The
  `-maxdepth` was wrong, so it matched nothing. **Zero findings and a broken
  check produce identical output.**

Both are the same failure: a scalar with no denominator, printed with the same
confidence whether it measured everything or nothing. Neither is fixed by
writing the script more carefully — the next one will make a third version of
it. It is fixed by rendering the fleet as a *grid you can look at*, where "this
column is empty because nothing was scanned" cannot look like "this column is
empty because everything is clean".

That is the entire justification. If the dashboard does not make those two
states visually distinct, it has failed and should not be built.

## Non-goals

- **Not part of the commit path.** The hook binary stays dependency-free; see
  "Packaging". A TUI in `pre-commit` would break GUI clients, CI, and piped
  output, and would seize the alternate screen during a commit.
- **Not a wrapper around the old migration scripts.** The scanner, previewer,
  and fixer are Rust code. `scripts/propagate.sh` is historical reference only
  and should disappear once the Rust path can prove the same removals/writes.
- **Not a git client.** No staging, committing, diffing. `lazygit` and `tig`
  exist and are better at it.
- **Not a linter runner.** It reports on hook *installation and configuration*,
  never on the content of your code.
- **Not a remote/CI dashboard.** Local `.git/hooks` state only. `gh dash`
  covers the remote side.

## Prior art worth stealing from

| Project | What to take |
|---|---|
| **k9s** | Resource table as the primary object; `/` to filter, `:` for a command palette; a context-sensitive hotkey footer that changes with the selected row. |
| **lazygit** | Contextual keymap always visible; panels that own their own keys; `?` for a full sheet. |
| **gh-dash** | The closest analogue — many remote items, sectioned, with per-row status glyphs and a detail pane. |
| **broot** | Incremental fuzzy filter that narrows as you type, with the match count always shown. |
| **btop / bottom** | Dense status rendering that stays legible at small widths. |
| **helix** | Selection → action ordering, and `which-key` style discoverability after a prefix. |
| **delta** | Restraint. Colour carries meaning, never decoration. |

## Design principles, and where they come from

**Overview first, zoom and filter, then details on demand.** Shneiderman's
visual information-seeking mantra (*The Eyes Have It*, 1996) is the literal
structure of this tool: a fleet summary band, a filterable repo table, a detail
pane on `Enter`. Do not invert it — no screen should open on a single repo.

**Visibility of system status** (Nielsen heuristic 1). A fleet scan takes ~8s
(2.9s to enumerate 98 `.git` directories, 4.9s to hash 96 shims). That is well
past the ~400ms Doherty threshold at which interaction stops feeling immediate,
so the scan MUST stream: rows appear as they are discovered, with a live
`scanned N/98` counter. A spinner over a blank screen is not acceptable.

**Recognition over recall** (Nielsen heuristic 6). The 20 check names are not
memorable. Anything the user must type — a check name, a repo path — is offered
as a filterable list first.

**Every count carries its denominator.** This is the local rule that follows
directly from the two failures above. Never render `0 drifted`; render
`0 drifted / 96 scanned`. A table with no rows renders as an explicit
diagnostic state, never as an empty success.

**Preattentive encoding, redundantly.** Colour and position are processed
pre-attentively (Ware, *Information Visualization*), which is what makes a grid
scannable at all. But roughly 8% of men have red-green colour vision deficiency,
so state is encoded **twice** — glyph *and* colour — and never by colour alone.
`NO_COLOR` must produce a fully usable screen.

**Data-ink ratio** (Tufte). No box-drawing around every cell, no gradients, no
progress bars where a number is clearer. Chrome competes with the data for the
same 80 columns.

## Data model

One row per repository. Collected read-only; the dashboard never writes without
an explicit action.

```
FleetScan {
  root             : PathBuf
  depth            : usize
  git_dirs_found   : usize
  hook_dirs_seen   : usize
  managed_seen     : usize
  unmanaged_seen   : usize
  unreadable       : Vec<PathBuf>
  excluded_dirs    : usize
  repos            : Vec<Repo>
}

Repo {
  path         : PathBuf        // displayed repo-relative to the scan root
  managed      : bool           // ≥1 shim dispatches to the binary
  shims        : [ShimState; 4] // commit-msg, pre-commit, pre-push, prepare-commit-msg
  stale_ours   : Vec<String>    // our old shims/check shims that are no longer shipped
  foreign_subs : Vec<String>    // hand-written pre-commit-* / pre-push-* sub-hooks
  hook_pkgjson : Option<String> // vestigial hooks/package.json from the node era
  baked        : BakeState      // which binary path the shims point at
  languages    : LangSet        // rust | js | python | k8s — from manifest presence
  skips        : Vec<String>    // git config --get-all hook.skip
}

ShimState = Ok            // installed bytes match the expected baked template
          | Drifted(hash) // present, but not the expected baked template
          | Missing
BakeState = Current       // == installed binary path
          | Stale(path)   // points somewhere else — the GUI-client failure mode
          | Unbaked       // __GITHOOKS_BIN__ placeholder intact
          | Mixed         // shims disagree with each other

FixPlan {
  repo    : PathBuf
  refuse  : Vec<Refusal> // tracked files, unreadable hooks dir, non-managed repo, ...
  remove  : Vec<Removal>
  write   : Vec<WriteShim>
}
```

`languages` matters because a check that never fires is not the same as a check
that is broken: `pre-commit-clippy` in a Python repo is *correctly* inert. The
dashboard must distinguish **inert** from **failing**, or it will manufacture 90
false problems out of `pre-commit-cargo-fmt`.

Shim comparison is against the rendered template for the intended binary path,
not the raw tracked template. A correctly baked shim must never be reported as
drifted merely because `__GITHOOKS_BIN__` was replaced.

## Screen 1 — Fleet overview (default)

```
┌ githooks fleet ──────────────────── scanning 98/98 · 8.1s ── /Users/me/Developer ┐
│ 96 managed · 2 unmanaged (skipped) · 384 shims · 20 checks                       │
│ consistency  commit-msg 96/1 ✓   pre-commit 96/1 ✓   pre-push 96/1 ✓             │
│              prepare-commit-msg 96/1 ✓          ← copies/distinct blobs          │
├─────────────────────────────────────────────────────────────────────────────────┤
│   REPO                       SHIMS  BAKE     LANG      SKIPS  WARN  STATE        │
│ ▸ Perso/homelab              ●●●●   current  k8s js    –      –     ✓ ok         │
│   Perso/application-landscape ●●●●  current  js        –      –     ✓ ok         │
│   Perso/git-templates        ●●●●   current  rust      –      –     ✓ ok         │
│   Perso/trade-agents         ●●●○   current  python    –      –     ✗ missing 1  │
│   Perso/frontjutsu/mvp       ●●●●   stale    js        2      1     ! stale bake │
│   Volkswagen/pos-fr-services ●●●●   current  js        –      –     ✓ ok         │
│   …                                                                              │
├─────────────────────────────────────────────────────────────────────────────────┤
│ 96 rows · 0 filtered   ↑↓ move  ⏎ detail  / filter  : command  h hooks  ? help   │
└─────────────────────────────────────────────────────────────────────────────────┘
```

- The **consistency band** is the headline, because `copies/distinct` is the one
  number that actually proves fleet health, and it is the number the text script
  got wrong. It is always `N/M`, never a bare adjective.
- `SHIMS` is four glyphs, one per dispatcher, in fixed order. `●` ok, `◐`
  drifted, `○` missing. Position encodes *which* hook without spending a column
  on its name.
- `STATE` is a redundant text summary of the same information, so the screen
  survives `NO_COLOR` and CVD.
- `DECL` counts the checks a repository declares in `.githooks.conf`, and reads
  `2!1` when one of those lines cannot be parsed — a check somebody committed
  that has never once run. `2` and `2!1` describing the same repository is the
  distinction the column exists for.
- `WARN` counts only the override git would actually APPLY. `--get-regexp`
  lists every configured entry, but the dispatcher asks `--get`, which returns
  the last — so a local `block` beats a global `warn`. Counting every entry made
  the column report a downgrade that never happens; the shadowed entry is still
  shown in the detail pane, marked `overridden`, because somebody wrote it.
- `SKIPS` and `WARN` are deliberately two columns, not one total. A skipped check
  does not run; a downgraded one runs, prints its failure, and lets the commit
  through. Summing them would hide the second — which is the one that looks like
  enforcement and is not. `WARN` counts only overrides that actually weaken
  something: an explicit `block`, a misspelt check name and an unrecognised value
  all change nothing, and inflating the count with them is how a column stops
  being read. The detail pane names all three.

### The empty state, which is the point

```
├─────────────────────────────────────────────────────────────────────────────────┤
│                                                                                 │
│   No repositories found under /Users/me/Dev                                     │
│                                                                                 │
│   Scanned 0 directories in 0.1s. This is a SCAN FAILURE, not a clean fleet.     │
│   • is --root correct?        (currently: /Users/me/Dev)                        │
│   • is --depth deep enough?   (currently: 6)                                    │
│                                                                                 │
```

An empty table must never render as a calm empty list. Zero scanned is stated as
a failure in words. This single screen is the reason the tool exists.

## Screen 2 — Repo detail (`Enter`)

```
┌ Perso/trade-agents ─────────────────────────────────────────────────────────────┐
│ /Users/me/Developer/Perso/trade-agents          python · managed · bake current │
├─────────────────────────────────────────────────────────────────────────────────┤
│ DISPATCHERS                                                                     │
│   ✓ commit-msg           blob 7914a85  matches template                         │
│   ✓ pre-commit           blob 7914a85  matches template                         │
│   ✓ pre-push             blob 7914a85  matches template                         │
│   ✗ prepare-commit-msg   MISSING       → `githooks fleet fix`                    │
│                                                                                 │
│ CHECKS (20)                        pre-commit                                   │
│   ● ban-terms      ● merge-conflict   ● package-lock   ● usual-name              │
│   ● lint-json-yaml ● yamllint         ○ lint-js        ○ prettier                │
│   ● ruff           ● pyright          ○ cargo-fmt      ○ clippy                  │
│   ○ argo-lint      ○ kube-linter      ○ kubeconform                             │
│                                    pre-push                                     │
│   ● branch-protect ● branch-pattern   ● pull-rebase    ○ run-tests-js            │
│   ○ cargo-test                                                                  │
│                                                                                 │
│   ● will run here   ○ inert (no matching manifest)   ⊘ skipped via hook.skip     │
├─────────────────────────────────────────────────────────────────────────────────┤
│ ⏎ back  s toggle skip  f fix this repo  o open  y copy path  ? help              │
└─────────────────────────────────────────────────────────────────────────────────┘
```

The `● / ○ / ⊘` distinction is the important one. A Python repo showing `○
clippy` is healthy; the same glyph must never be used for "broken". Three states,
three glyphs, three words in the legend — no colour required to read it.

## Screen 3 — Hook-centric view (`h`)

Transposes the matrix. Answers "where does `pre-commit-pyright` actually apply,
run, get skipped, or stay inert?" Checks are no longer installed as per-repo
files; the view is about runtime applicability, not file presence.

```
│   CHECK                 APPLICABLE  ACTIVE   SKIPPED   INERT                    │
│ ▸ pre-commit-ban-terms      96        96        0        0                      │
│   pre-commit-ruff           11        11        0       85                      │
│   pre-commit-pyright        11        11        0       85                      │
│   pre-commit-clippy          3         3        0       93                      │
│   pre-push-run-tests-js     41        39        2       55                      │
```

Rows sum across; `APPLICABLE = ACTIVE + SKIPPED`. A row where `APPLICABLE` is 0
is highlighted, because a check that can never fire anywhere is either dead or
misconfigured — and that is invisible in today's text output.

## Interaction model

Modal and keyboard-first, following k9s and helix. No mouse dependency; mouse
scroll may be supported but nothing is mouse-only.

| Key | Action |
|---|---|
| `↑↓` `jk` | move selection |
| `Enter` | detail of selected row |
| `Esc` | back / clear filter |
| `/` | incremental filter (path, language, state) — match count always shown |
| `:` | command palette (`:fix`, `:rescan`, `:root <path>`, `:export json`) |
| `h` | toggle fleet ↔ hook-centric view |
| `s` | toggle `hook.skip` for the selected check (detail view) |
| `f` | fix selected repo — **always previews, never applies silently** |
| `r` | rescan |
| `?` | full key sheet |
| `q` | quit |

**Destructive actions are diff-first.** `f` opens a confirmation listing exactly
what would be removed and written, per file, from a typed Rust `FixPlan`.
Nothing in this tool deletes a file without showing the list and taking a second
keypress. Given `make install` has destroyed tracked source twice in this repo's
history, the dashboard's write path gets the same fail-closed treatment: it
refuses any path git reports as tracked.

## Performance

- **Stream, never batch.** Enumerate `.git` directories first (2.9s) and paint
  rows immediately in `scanning` state; hash shims on a worker pool and update
  rows in place. Sorting is stable so rows do not jump under the cursor.
- **Never block the UI thread.** A 16ms frame budget; scanning runs on threads
  and posts results over a channel. `q` must work during a scan.
- **Bounded work.** Respect `--depth` (default 6) and skip `node_modules`,
  `target`, `.venv`, `vendor` — the same exclusions the existing sweep uses.
- **No cache in v1.** An 8s scan does not justify a cache and its invalidation
  bugs. Revisit only if the fleet grows past a few hundred repos.

## Accessibility and degradation

- **`NO_COLOR`** (and `TERM=dumb`) → glyph-and-text rendering, fully usable.
- **CVD-safe**: state never encoded by colour alone; every colour is paired with
  a distinct glyph and a word in the legend.
- **Narrow terminals**: below 100 columns drop `LANG`, `SKIPS`, `WARN` and
  `DECL` together; below 60, fall back to a single-column list. Never horizontal
  scrolling.
- **Screen readers do not meaningfully work with TUIs.** The accessible path is
  therefore `githooks fleet --json`, emitting the full data model for scripting
  and assistive tooling. This is a first-class output, not a debug flag, and the
  TUI is a renderer over it.

## Packaging

A cargo **workspace**, so the commit path keeps its posture:

```
crates/
  githooks-runtime/ # registry + hook logic. std-only.
  githooks/         # the hook binary. ZERO external dependencies.
  githooks-fleet/   # scanner, fixer, JSON, TUI. ratatui + crossterm.
```

`githooks` must not gain an external dependency, directly or transitively; that
property is the reason the Rust migration happened at all. The TUI is a separate
artifact that a developer opts into, and `make install` continues to install
only the hook binary.

Extracting `githooks-runtime` has an independent benefit: there is currently no
lib target, which is why `cargo test --lib` fails outright. Unit tests for hook
logic would move into a library where they belong, without letting TUI
dependencies into the commit path.

## Version plan

### v1 — Rust-native fleet truth

Build the minimum useful tool, but build it in the final architecture:

- Workspace split into `githooks-runtime`, `githooks`, and `githooks-fleet`.
- Rust scanner for `--root` and `--depth`, with explicit counters for found git
  dirs, seen hook dirs, managed/unmanaged repos, unreadable paths, and excluded
  directories.
- Rust shim inspection: four dispatcher states, baked path state, stale managed
  files, foreign sub-hooks, vestigial hook `package.json`, languages, skips.
- `githooks-fleet --json`, emitting `FleetScan`.
- Default TUI overview and repo detail views.
- Rust `githooks-fleet fix [repo|--all] --dry-run`, producing a `FixPlan`.
- Rust apply path for that exact `FixPlan`, with a second confirmation in the
  TUI and tracked-file refusal before any remove/write.
- Tests for broken root, too-shallow depth, baked-template comparison, managed
  detection, stale file classification, tracked-file refusal, and dry-run/apply
  parity.

v1 may omit the hook-centric view and skip toggling if they slow down the first
usable release. It may not shell out to `propagate.sh`.

### v2 — operational dashboard

Add the workflows that make the grid more than a safer propagation report:

- Hook-centric view (`h`) over applicability, active, skipped, and inert counts.
- Detail drilldown from a check row to the matching repos.
- `s` toggle for `hook.skip`, backed by Rust `git config` calls and a preview of
  the exact config mutation.
- Command palette entries for `:root`, `:depth`, `:rescan`, `:export json`, and
  `:fix`.
- Optional activity signal after measuring cost: last commit date or another
  cheap recency marker, never on the initial UI thread.
- Remove `scripts/propagate.sh` once v1/v2 fix coverage has replaced its last
  practical use.

## Decisions

1. **Scan root.** Use a CLI flag in v1. Add persistent config only after using
   the tool enough to know where it belongs.
2. **Fix path.** Native Rust. Do not shell out to `propagate.sh`.
3. **Delivery target.** Build through v2. v1 is the first shippable slice, not
   the end state.
4. **Activity signal.** Sorting by last-commit date would surface "the repos you
   actually use are drifted" — but it costs a `git log` per repo. Measure first.

## Delivery plan

Seven PRs. The ordering is not cosmetic: the destructive code is written only
after a differential has proven the model it destroys with, and the TUI is last
because it is the least risky part and the least useful if the data beneath it
is wrong.

**1. Workspace split, zero behaviour change.** `githooks-runtime` (lib,
std-only) and `githooks` (bin), no features. The proof is that all 174 tests
pass UNCHANGED, plus a differential running every hook over the same fixtures
before and after, comparing bytes and exit codes — the technique that caught
four bugs during the zsh port. This PR must also land the **zero-dependency CI
guard** (`cargo tree` for `githooks` shows nothing external); without it the
packaging rule is a comment, and ratatui arrives transitively three PRs later.
Highest mechanical risk, no feature value, therefore alone.

**2. Scanner and `--json`, no TUI.** `FleetScan` with every counter. This
already replaces the ad-hoc `find`/`hash-object` pipelines that misled twice, so
it is useful before any UI exists. Tests: broken root, too-shallow depth,
excluded directories counted, unreadable paths recorded.

**3. Shim classification.** Expected shim = the embedded template rendered for
the intended binary path. The trap is symmetrical and silent in both
directions: compare against the RAW template and all 96 repos report drifted, so
the tool cries wolf and gets ignored; compare too loosely and real drift is
never seen. Both directions get a test.

**4. `FixPlan` dry-run and the parity gate.** A differential over all 96 repos:
the Rust plan against `propagate.sh --dry-run`, normalised and compared. This is
what earns the right to delete the shell script, and it is the long pole of the
project — not the TUI.

**5. Apply path, fail closed.** Refuses tracked paths, unmanaged repos and
unreadable hook directories. Dry-run/apply parity plus idempotence: applying
twice must produce an empty second plan. Written only after 4 proves the model.

**6. TUI v1 — overview and detail.** ratatui's `TestBackend` renders into an
assertable buffer, so the success criterion becomes an automated test: a
deliberately broken scan must render *SCAN FAILURE*. Same for `NO_COLOR` and the
60/100-column fallbacks. A success criterion nobody can run is a wish.

**7. v2, then remove `propagate.sh`.** Hook-centric view, `s` skip toggle,
command palette. The script is deleted once 4 and 5 have replaced its last
practical use.

### Two details settled before starting

- **Invocation is `githooks-fleet`, not `githooks fleet`.** A subcommand would
  require the hook binary to locate and exec the TUI binary, coupling the commit
  path to a tool it must never know about. The screens' `githooks fleet` prompt
  is shorthand for the separate binary.
- **Templates are embedded** with `include_str!` against the workspace root, so
  the tool reports correctly from any directory rather than only inside a
  checkout. All four dispatcher templates are currently ONE blob, so this is a
  single embedded string, not four.

## Success criteria

The tool is worth building only if, on a deliberately broken scan (wrong root,
wrong depth), the screen says *scan failure* rather than showing a clean, empty,
green fleet. Everything else is convenience.
