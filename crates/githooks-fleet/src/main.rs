//! `githooks-fleet` — what the hook fleet actually looks like on this machine.
//!
//! A separate binary from `githooks` on purpose. This one may have
//! dependencies; the hook binary may not, because it runs on every commit in
//! every repo. See `scripts/check-no-deps.sh`, which enforces that in CI.
//!
//! This release is the scanner and `--json` only. The TUI will render over
//! exactly this data, so the JSON is the contract rather than a debug
//! afterthought — and it is also the accessible path, since screen readers do
//! not meaningfully work with a TUI.

mod apply;
mod checks;
mod fix;
mod scan;
mod severities;
mod shim;
mod skips;
mod tui;

use std::path::{Path, PathBuf};
use std::process::ExitCode;

const USAGE: &str = "\
usage: githooks-fleet [scan|tui|fix|install|uninstall] [--root <dir>] [--depth <n>] [--json]

  scan           report the fleet (default)
  tui            the interactive dashboard
  install        turn hooks on across the root
  uninstall      take OUR shims back out — never a hook somebody else wrote,
                 and never hook.skip or githooks.severity
  fix            show what would be changed — DRY RUN unless --apply
  fix --apply    carry out the plan

  --root <dir>   where to look for repositories (default: $HOME/Developer)
  --depth <n>    directory levels to descend  (default: 6)
  --binary <p>   the binary shims should point at (default: $HOME/.local/bin/githooks)
  --json         emit the result as JSON
";

#[derive(PartialEq)]
enum Mode {
    Scan,
    Fix,
    Tui,
    /// Turn hooks ON across a root. Named after intent, unlike `fix --apply`,
    /// which does the same writing but reads as repair — which is why nobody
    /// reached for it when they meant "set this up".
    Install,
    /// And off again.
    Uninstall,
}

struct Args {
    mode: Mode,
    root: PathBuf,
    depth: usize,
    json: bool,
    apply: bool,
    /// Which binary the shims should point at. Overridable because an install
    /// is not always at the default path, and because a comparison is only
    /// meaningful when both sides agree on the target.
    binary: Option<String>,
}

/// `home` is threaded through rather than read from the environment here, so
/// the no-`$HOME`-and-no-`--root` refusal below is a plain unit test rather
/// than something that can only be exercised by mutating the real process
/// environment (racy, since tests in this binary run in parallel).
fn parse(argv: &[String], home: Option<&Path>) -> Result<Args, String> {
    let mut mode = Mode::Scan;
    let mut root: Option<PathBuf> = None;
    let mut depth = 6;
    let mut json = false;
    let mut apply = false;
    let mut binary: Option<String> = None;

    let mut it = argv.iter().peekable();
    if let Some(first) = it.peek() {
        match first.as_str() {
            "scan" => {
                mode = Mode::Scan;
                it.next();
            }
            "fix" => {
                mode = Mode::Fix;
                it.next();
            }
            "tui" => {
                mode = Mode::Tui;
                it.next();
            }
            "install" => {
                mode = Mode::Install;
                // Installing IS applying; requiring `--apply` as well would be
                // asking twice for one decision.
                apply = true;
                it.next();
            }
            "uninstall" => {
                mode = Mode::Uninstall;
                apply = true;
                it.next();
            }
            _ => {}
        }
    }
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--json" => json = true,
            "--root" => {
                root = Some(it.next().ok_or("--root needs a directory")?.into());
            }
            "--depth" => {
                let v = it.next().ok_or("--depth needs a number")?;
                depth = v
                    .parse()
                    .map_err(|_| format!("--depth: {v:?} is not a number"))?;
            }
            "--apply" => apply = true,
            "--binary" => {
                binary = Some(it.next().ok_or("--binary needs a path")?.clone());
            }
            "-h" | "--help" => return Err(String::new()),
            other => return Err(format!("unknown argument {other:?}")),
        }
    }
    // Resolved last, so it only runs when no explicit `--root` makes it moot.
    let root = match root {
        Some(r) => r,
        None => default_root(home).ok_or_else(|| {
            "no --root given and $HOME is not set — refusing to guess \
             (the alternative is silently scanning, and `fix --apply`ing, \
             from wherever this happened to be launched)"
                .to_string()
        })?,
    };
    Ok(Args {
        mode,
        root,
        depth,
        json,
        apply,
        binary,
    })
}

fn home() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}

/// Where `make install` puts the binary. Shims baked with anything else are
/// reported as stale, which is the GUI-client failure mode.
///
/// `home: None` has no good guess and must not pretend to: a bare relative
/// `"githooks"` baked into a shim is a silently broken install — the same
/// failure family as #79 (a wrong path from a missing env var), just on the
/// write side rather than the delete side.
fn default_binary(home: Option<&Path>) -> Option<String> {
    home.map(|h| h.join(".local/bin/githooks").to_string_lossy().into_owned())
}

/// `home: None` must not fall back to `.` — `fix --apply`/`install` would
/// then operate, recursively, from wherever the process happened to be
/// launched, with only `is_dir()` standing between that and doing real work.
/// #79 was exactly this: a surprising path from an absent env var.
fn default_root(home: Option<&Path>) -> Option<PathBuf> {
    home.map(|h| h.join("Developer"))
}

fn main() -> ExitCode {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let args = match parse(&argv, home().as_deref()) {
        Ok(a) => a,
        Err(e) => {
            if !e.is_empty() {
                eprintln!("githooks-fleet: {e}");
            }
            eprint!("{USAGE}");
            return ExitCode::from(2);
        }
    };

    if !args.root.is_dir() {
        eprintln!(
            "githooks-fleet: --root {} is not a directory",
            args.root.display()
        );
        return ExitCode::from(2);
    }

    let started = std::time::Instant::now();
    let installed = match args
        .binary
        .clone()
        .or_else(|| default_binary(home().as_deref()))
    {
        Some(b) => b,
        None => {
            eprintln!(
                "githooks-fleet: no --binary given and $HOME is not set — \
                 refusing to guess which binary the shims should point at"
            );
            return ExitCode::from(2);
        }
    };

    if args.mode == Mode::Tui {
        return match tui::run(args.root.clone(), args.depth, installed.clone()) {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("githooks-fleet: {e}");
                ExitCode::from(1)
            }
        };
    }

    let scan = scan::scan(&args.root, args.depth, &installed);
    let elapsed = started.elapsed();

    if args.mode == Mode::Uninstall {
        let mut removed = 0usize;
        let mut repos = 0usize;
        for repo in scan.repos.iter().filter(|r| r.managed) {
            let hooks = scan::hooks_dir_for(&args.root.join(&repo.path));
            let mut here = 0usize;
            for name in githooks_runtime::install::DISPATCHERS {
                let path = hooks.join(name);
                // Only ours. A hook somebody wrote is not ours to take, and
                // this is the guard whose absence let `install` clobber one.
                let ours = std::fs::read_to_string(&path)
                    .map(|t| githooks_runtime::install::is_our_shim(&t))
                    .unwrap_or(false);
                // AND not tracked: the same guard `fix`/`apply` treat as
                // mandatory before any removal, for the same reason —
                // `.git/hooks` reached through a symlinked template dir can
                // resolve into a tracked checkout, and `is_our_shim` alone
                // does not know that. This path deletes independently of
                // `fix::plan`/`apply::apply`, so it must repeat the check
                // rather than rely on going through them.
                if ours && !fix::is_tracked(&path) && std::fs::remove_file(&path).is_ok() {
                    here += 1;
                }
            }
            if here > 0 {
                repos += 1;
                removed += here;
                println!("  {} {here} shims", repo.path.display());
            }
        }
        println!("{removed} shims removed from {repos} repositories");
        println!("hook.skip and githooks.severity were left alone.");
        return ExitCode::SUCCESS;
    }

    if args.mode == Mode::Fix || args.mode == Mode::Install {
        let plans: Vec<fix::FixPlan> = scan
            .repos
            .iter()
            .map(|r| {
                let intent = if args.mode == Mode::Install {
                    fix::Intent::Activate
                } else {
                    fix::Intent::Repair
                };
                fix::plan(r, &args.root.join(&r.path), &installed, intent)
            })
            .collect();
        if args.apply {
            let reports: Vec<apply::ApplyReport> = plans
                .iter()
                .map(|p| apply::ApplyReport {
                    repo: p.repo.clone(),
                    outcome: apply::apply(p),
                })
                .collect();
            if args.json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&reports).unwrap_or_default()
                );
            } else {
                report_apply(&reports);
            }
            let failed = reports
                .iter()
                .any(|r| matches!(r.outcome, apply::Outcome::Failed { .. }));
            return if failed {
                ExitCode::from(1)
            } else {
                ExitCode::SUCCESS
            };
        }
        if args.json {
            println!(
                "{}",
                serde_json::to_string_pretty(&plans).unwrap_or_default()
            );
        } else {
            report_fix(&plans);
        }
        return if scan.looks_like_a_failed_scan() {
            ExitCode::from(1)
        } else {
            ExitCode::SUCCESS
        };
    }

    if args.json {
        match serde_json::to_string_pretty(&scan) {
            Ok(s) => println!("{s}"),
            Err(e) => {
                eprintln!("githooks-fleet: cannot serialise scan: {e}");
                return ExitCode::from(1);
            }
        }
    } else {
        report(&scan, elapsed);
    }

    // A scan that found nothing exits NON-ZERO. "No repositories" has to be
    // actionable from a script as well as on screen, because the failure this
    // tool exists for is exactly that emptiness reading as success.
    if scan.looks_like_a_failed_scan() {
        return ExitCode::from(1);
    }
    ExitCode::SUCCESS
}

/// Every number carries its denominator. No bare adjectives.
fn report(s: &scan::FleetScan, elapsed: std::time::Duration) {
    if s.looks_like_a_failed_scan() {
        println!("No repositories found under {}", s.root.display());
        println!();
        println!(
            "Visited {} directories in {:.1}s and found 0 git repositories.",
            s.dirs_visited,
            elapsed.as_secs_f64()
        );
        println!("This is a SCAN FAILURE, not a clean fleet.");
        println!(
            "  • is --root correct?       (currently: {})",
            s.root.display()
        );
        println!("  • is --depth deep enough?  (currently: {})", s.depth);
        if !s.unreadable.is_empty() {
            println!("  • {} path(s) could not be read", s.unreadable.len());
        }
        return;
    }

    println!("{}", s.root.display());
    println!(
        "  {} git repositories · {} managed · {} unmanaged",
        s.git_dirs_found, s.managed_seen, s.unmanaged_seen
    );
    println!(
        "  {} hook directories · {} directories visited · {} subtrees skipped · {:.1}s",
        s.hook_dirs_seen,
        s.dirs_visited,
        s.excluded_dirs,
        elapsed.as_secs_f64()
    );
    if !s.unreadable.is_empty() {
        println!("  {} unreadable:", s.unreadable.len());
        for p in s.unreadable.iter().take(5) {
            println!("    {}", p.display());
        }
    }
}

/// Dry-run summary. Counts carry denominators, and a refusal is never folded
/// into the same number as a change.
fn report_fix(plans: &[fix::FixPlan]) {
    let total = plans.len();
    let refused: Vec<&fix::FixPlan> = plans.iter().filter(|p| p.refused()).collect();
    let acting: Vec<&fix::FixPlan> = plans
        .iter()
        .filter(|p| !p.refused() && !p.is_noop())
        .collect();

    for p in &acting {
        println!("{}", p.repo.display());
        for r in &p.remove {
            println!("  rm    {}  ({:?})", r.path.display(), r.reason);
        }
        for w in p.write.iter().filter(|w| w.changes) {
            println!("  write {}", w.path.display());
        }
    }

    println!();
    println!(
        "  {} of {} repositories would change · {} refused · {} already correct",
        acting.len(),
        total,
        refused.len(),
        total - acting.len() - refused.len()
    );
    println!(
        "  {} removals · {} writes",
        acting.iter().map(|p| p.remove.len()).sum::<usize>(),
        acting
            .iter()
            .map(|p| p.write.iter().filter(|w| w.changes).count())
            .sum::<usize>()
    );
    println!();
    println!("  DRY RUN — nothing was written.");
}

/// What actually happened. A failure is never folded into the same count as a
/// success, and "refused" is reported separately from "unchanged" — they look
/// identical in a total and mean opposite things.
fn report_apply(reports: &[apply::ApplyReport]) {
    let mut applied = 0usize;
    let (mut removed, mut written, mut refused, mut unchanged) = (0usize, 0usize, 0usize, 0usize);
    for r in reports {
        match &r.outcome {
            apply::Outcome::Applied {
                removed: rm,
                written: wr,
            } => {
                applied += 1;
                removed += rm;
                written += wr;
                println!("{}  -{rm} +{wr}", r.repo.display());
            }
            apply::Outcome::Refused => refused += 1,
            apply::Outcome::Unchanged => unchanged += 1,
            apply::Outcome::Failed { error, at } => {
                println!("{}  FAILED at {at}: {error}", r.repo.display());
            }
        }
    }
    let failed = reports
        .iter()
        .filter(|r| matches!(r.outcome, apply::Outcome::Failed { .. }))
        .count();
    println!();
    println!(
        "  {applied} of {} repositories changed · {refused} refused · {unchanged} already correct · {failed} failed",
        reports.len()
    );
    println!("  {removed} removed · {written} written");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_flags_and_defaults() {
        let home = Path::new("/home/x");
        let a = parse(&[], Some(home)).expect("defaults");
        assert_eq!(a.depth, 6);
        assert!(!a.json);
        assert_eq!(a.root, home.join("Developer"));

        let a = parse(&["--depth".into(), "2".into(), "--json".into()], Some(home)).unwrap();
        assert_eq!(a.depth, 2);
        assert!(a.json);
    }

    #[test]
    fn rejects_bad_input_loudly() {
        let home = Some(Path::new("/home/x"));
        assert!(parse(&["--depth".into(), "lots".into()], home).is_err());
        assert!(parse(&["--depth".into()], home).is_err());
        assert!(parse(&["--nope".into()], home).is_err());
    }

    /// The failure this exists to prevent: `$HOME` missing and no `--root`
    /// silently resolving to `.` — `fix --apply`/`install` would then act,
    /// recursively, on whatever directory the process happened to be
    /// launched from. #79 was exactly this shape, on the delete side.
    #[test]
    fn no_home_and_no_root_is_refused_not_guessed() {
        assert!(
            parse(&[], None).is_err(),
            "must refuse rather than default to the current directory"
        );
    }

    /// An explicit `--root` moots the whole question — `$HOME` need not even
    /// exist for this to work.
    #[test]
    fn an_explicit_root_needs_no_home() {
        let a = parse(&["--root".into(), "/somewhere".into()], None).expect("explicit --root");
        assert_eq!(a.root, PathBuf::from("/somewhere"));
    }

    #[test]
    fn default_root_and_binary_need_home() {
        assert_eq!(
            default_root(None),
            None,
            "no directory to guess without $HOME"
        );
        assert_eq!(
            default_binary(None),
            None,
            "no binary path to guess without $HOME"
        );

        let home = Path::new("/home/x");
        assert_eq!(default_root(Some(home)), Some(home.join("Developer")));
        assert_eq!(
            default_binary(Some(home)),
            Some(
                home.join(".local/bin/githooks")
                    .to_string_lossy()
                    .into_owned()
            )
        );
    }
}
