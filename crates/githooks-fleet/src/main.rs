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

use std::path::PathBuf;
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
  --agents-md    with fix/install: also roll out the AGENTS.md pointer
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
    /// Roll the AGENTS.md pointer out alongside fix/install. Opt-in per
    /// invocation, never bundled into `--apply` by default — writing tracked
    /// content across up to 96 repositories is a materially bigger action
    /// than the untracked `.git/hooks` shims apply already writes.
    agents_md: bool,
}

fn parse(argv: &[String]) -> Result<Args, String> {
    let mut a = Args {
        mode: Mode::Scan,
        root: default_root(),
        depth: 6,
        json: false,
        apply: false,
        binary: None,
        agents_md: false,
    };
    let mut it = argv.iter().peekable();
    if let Some(first) = it.peek() {
        match first.as_str() {
            "scan" => {
                a.mode = Mode::Scan;
                it.next();
            }
            "fix" => {
                a.mode = Mode::Fix;
                it.next();
            }
            "tui" => {
                a.mode = Mode::Tui;
                it.next();
            }
            "install" => {
                a.mode = Mode::Install;
                // Installing IS applying; requiring `--apply` as well would be
                // asking twice for one decision.
                a.apply = true;
                it.next();
            }
            "uninstall" => {
                a.mode = Mode::Uninstall;
                a.apply = true;
                it.next();
            }
            _ => {}
        }
    }
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--json" => a.json = true,
            "--root" => {
                a.root = it.next().ok_or("--root needs a directory")?.into();
            }
            "--depth" => {
                let v = it.next().ok_or("--depth needs a number")?;
                a.depth = v
                    .parse()
                    .map_err(|_| format!("--depth: {v:?} is not a number"))?;
            }
            "--apply" => a.apply = true,
            "--agents-md" => a.agents_md = true,
            "--binary" => {
                a.binary = Some(it.next().ok_or("--binary needs a path")?.clone());
            }
            "-h" | "--help" => return Err(String::new()),
            other => return Err(format!("unknown argument {other:?}")),
        }
    }
    Ok(a)
}

/// Where `make install` puts the binary. Shims baked with anything else are
/// reported as stale, which is the GUI-client failure mode.
fn default_binary() -> String {
    std::env::var_os("HOME")
        .map(|h| PathBuf::from(h).join(".local/bin/githooks"))
        .unwrap_or_else(|| PathBuf::from("githooks"))
        .to_string_lossy()
        .into_owned()
}

fn default_root() -> PathBuf {
    std::env::var_os("HOME")
        .map(|h| PathBuf::from(h).join("Developer"))
        .unwrap_or_else(|| PathBuf::from("."))
}

fn main() -> ExitCode {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let args = match parse(&argv) {
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
    let installed = args.binary.clone().unwrap_or_else(default_binary);

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
            let hooks = args.root.join(&repo.path).join(".git/hooks");
            let mut here = 0usize;
            for name in githooks_runtime::install::DISPATCHERS {
                let path = hooks.join(name);
                // Only ours. A hook somebody wrote is not ours to take, and
                // this is the guard whose absence let `install` clobber one.
                let ours = std::fs::read_to_string(&path)
                    .map(|t| githooks_runtime::install::is_our_shim(&t))
                    .unwrap_or(false);
                if ours && std::fs::remove_file(&path).is_ok() {
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
                fix::plan(
                    r,
                    &args.root.join(&r.path),
                    &installed,
                    intent,
                    args.agents_md,
                )
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
        if let Some(w) = &p.write_agents_md {
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
            .map(|p| p.write.iter().filter(|w| w.changes).count()
                + usize::from(p.write_agents_md.is_some()))
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
        let a = parse(&[]).expect("defaults");
        assert_eq!(a.depth, 6);
        assert!(!a.json);

        let a = parse(&["--depth".into(), "2".into(), "--json".into()]).unwrap();
        assert_eq!(a.depth, 2);
        assert!(a.json);
    }

    #[test]
    fn rejects_bad_input_loudly() {
        assert!(parse(&["--depth".into(), "lots".into()]).is_err());
        assert!(parse(&["--depth".into()]).is_err());
        assert!(parse(&["--nope".into()]).is_err());
    }
}
