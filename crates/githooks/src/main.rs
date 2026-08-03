//! The hook binary — argument handling only.
//!
//! Everything it does lives in `githooks-runtime`. Git invokes exactly four
//! hook names; the shim passes its own filename through, and the registry maps
//! that name to a handler.
//!
//! ```text
//! githooks --hooks-dir <dir> <hook-name> [args…]
//! githooks list [--json] [--stage pre-commit|pre-push] [--pushed]
//! githooks install | uninstall [--binary] | trust [--show|--revoke]
//! githooks run [<check>] [--all-files] | restore
//! githooks agents-md [--check] [--path <file>]
//! ```

use std::ffi::OsString;
use std::path::PathBuf;

use githooks_runtime::check::Stage;
use githooks_runtime::{pushrefs, registry, ListOptions};

fn main() {
    let mut args = std::env::args_os().skip(1);
    let mut hooks_dir: Option<PathBuf> = None;
    let mut hook: Option<String> = None;
    let mut rest: Vec<OsString> = Vec::new();

    while let Some(a) = args.next() {
        match a.to_str() {
            Some("--hooks-dir") => {
                hooks_dir = args.next().map(PathBuf::from);
            }
            _ if hook.is_none() => {
                hook = a.to_str().map(str::to_owned);
            }
            _ => {
                rest.push(a);
                rest.extend(args.by_ref());
                break;
            }
        }
    }

    // `githooks list` — what would run here, and why. Lives in the HOOK binary
    // rather than the fleet tool because this is the binary installed
    // everywhere, and the question is asked about the repo you are standing in.
    if rest.first().is_some_and(|a| a == "list") || hook.as_deref() == Some("list") {
        let json = rest.iter().any(|a| a == "--json");
        let pushed = rest.iter().any(|a| a == "--pushed");
        let stage = match stage_flag(&rest) {
            Ok(s) => s,
            Err(msg) => {
                eprintln!("githooks: {msg}");
                std::process::exit(2);
            }
        };
        let code = githooks_runtime::list_checks(ListOptions {
            json,
            stage,
            pushed,
        });
        std::process::exit(code);
    }

    // `githooks install` — was a Makefile recipe. It lives here so the guard
    // that decides whether a directory may be emptied has ONE implementation,
    // tested on every platform, rather than one in `make` and another in
    // PowerShell for the Windows users who have no `make` at all.
    if rest.first().is_some_and(|a| a == "restore") || hook.as_deref() == Some("restore") {
        std::process::exit(match githooks_runtime::staged_only::restore_command() {
            Ok(()) => 0,
            Err(e) => {
                eprintln!("{e}");
                1
            }
        });
    }

    if rest.first().is_some_and(|a| a == "run") || hook.as_deref() == Some("run") {
        let all_files = rest.iter().any(|a| a == "--all-files");
        // A named check runs alone; otherwise the whole pre-commit stage.
        let named = rest
            .iter()
            .filter_map(|a| a.to_str())
            .find(|a| !a.starts_with("--") && *a != "run")
            .map(str::to_owned);
        let push = if named.as_deref().is_some_and(needs_synthetic_push_refs) {
            // A pre-push check invoked standalone has no real push on the
            // other end of stdin to read refs from. Left as `::default()`,
            // `.get()` would either block on a TTY that has nothing coming,
            // or (stdin redirected from `/dev/null`) read an empty ref list
            // — which every pre-push check treats as "nothing to check" and
            // passes, silently. Synthesize the one ref a standalone run can
            // still answer for: what HEAD would push.
            match pushrefs::synthetic_from_upstream() {
                Ok(r) => pushrefs::PushRefs::preloaded(vec![r]),
                Err(e) => {
                    eprintln!("githooks: {e}");
                    std::process::exit(2);
                }
            }
        } else {
            pushrefs::PushRefs::default()
        };
        let hooks_dir = hooks_dir
            .clone()
            .unwrap_or_else(|| PathBuf::from(".git/hooks"));
        let name = named.clone().unwrap_or_else(|| "pre-commit".to_string());
        let ctx = registry::Ctx {
            name: &name,
            args: &[],
            hooks_dir: &hooks_dir,
            push: &push,
        };
        if all_files {
            let files = std::process::Command::new("git")
                .args(["ls-files"])
                .output()
                .ok()
                .map(|o| {
                    String::from_utf8_lossy(&o.stdout)
                        .lines()
                        .map(str::to_owned)
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            githooks_runtime::hooks::common::override_file_set(files);
        }
        let verdict = match named {
            Some(check) => match registry::lookup(&check) {
                Some(run_check) => run_check(&ctx),
                None => {
                    eprintln!("githooks: unknown check {check:?} — try `githooks list`");
                    std::process::exit(2);
                }
            },
            None => githooks_runtime::dispatch::run_all(&ctx, all_files),
        };
        std::process::exit(verdict.exit_code());
    }

    if rest.first().is_some_and(|a| a == "trust") || hook.as_deref() == Some("trust") {
        std::process::exit(match githooks_runtime::trust::command(&rest) {
            Ok(()) => 0,
            Err(e) => {
                eprintln!("{e}");
                1
            }
        });
    }

    if rest.first().is_some_and(|a| a == "agents-md") || hook.as_deref() == Some("agents-md") {
        let check_only = rest.iter().any(|a| a == "--check");
        let path = path_flag(&rest).unwrap_or_else(|| {
            PathBuf::from(githooks_runtime::hooks::common::repo_root()).join("AGENTS.md")
        });
        let code = if check_only {
            match githooks_runtime::agents_md::check(&path) {
                Ok(githooks_runtime::agents_md::CheckResult::NotPresent) => {
                    println!(
                        "{}: not present (opt-in — run without --check to add it)",
                        path.display()
                    );
                    0
                }
                Ok(githooks_runtime::agents_md::CheckResult::MatchesGenerated) => {
                    println!("{}: up to date", path.display());
                    0
                }
                Ok(githooks_runtime::agents_md::CheckResult::Drifted) => {
                    eprintln!(
                        "{}: drifted from the generated block — run `githooks agents-md`",
                        path.display()
                    );
                    1
                }
                Err(e) => {
                    eprintln!("{e}");
                    1
                }
            }
        } else {
            match githooks_runtime::agents_md::write(&path) {
                Ok(()) => {
                    println!("wrote {}", path.display());
                    0
                }
                Err(e) => {
                    eprintln!("{e}");
                    1
                }
            }
        };
        std::process::exit(code);
    }

    if rest.first().is_some_and(|a| a == "uninstall") || hook.as_deref() == Some("uninstall") {
        let also_binary = rest.iter().any(|a| a == "--binary");
        std::process::exit(match githooks_runtime::install::uninstall(also_binary) {
            Ok(()) => 0,
            Err(e) => {
                eprintln!("{e}");
                1
            }
        });
    }

    if rest.first().is_some_and(|a| a == "install") || hook.as_deref() == Some("install") {
        std::process::exit(
            match githooks_runtime::install::run(rest.iter().any(|a| a == "--force")) {
                Ok(()) => 0,
                Err(e) => {
                    eprintln!("{e}");
                    1
                }
            },
        );
    }

    let (Some(hooks_dir), Some(hook)) = (hooks_dir, hook) else {
        eprintln!("usage: githooks --hooks-dir <dir> <hook-name> [args…]");
        std::process::exit(2);
    };

    let push = pushrefs::PushRefs::default();
    let ctx = registry::Ctx {
        name: &hook,
        args: &rest,
        hooks_dir: &hooks_dir,
        push: &push,
    };
    // THE process boundary: the one place a hook result becomes a number.
    // Everything above speaks `Verdict`, and 2 is neither of its answers —
    // it means the binary was invoked wrongly, not that a hook decided
    // anything, which is why it is written here and nowhere else.
    const USAGE_ERROR: i32 = 2;
    let code = match registry::lookup(&hook) {
        Some(run_hook) => run_hook(&ctx).exit_code(),
        None => {
            eprintln!("githooks: unknown hook {hook:?}");
            USAGE_ERROR
        }
    };
    std::process::exit(code);
}

/// `--stage <pre-commit|pre-push>` for `list`, ad hoc scanned like every other
/// flag here — no parsing library, matching `trust.rs`'s `let flag = |f: &str|
/// args.iter().any(|a| a == f);` idiom.
fn stage_flag(rest: &[OsString]) -> Result<Option<Stage>, String> {
    let mut iter = rest.iter();
    while let Some(a) = iter.next() {
        if a == "--stage" {
            let v = iter
                .next()
                .and_then(|v| v.to_str())
                .ok_or_else(|| "--stage requires a value".to_string())?;
            return match v {
                "pre-commit" => Ok(Some(Stage::PreCommit)),
                "pre-push" => Ok(Some(Stage::PrePush)),
                other => Err(format!(
                    "--stage must be `pre-commit` or `pre-push`, got {other:?}"
                )),
            };
        }
    }
    Ok(None)
}

/// `--path <file>` for `agents-md`.
fn path_flag(rest: &[OsString]) -> Option<PathBuf> {
    let mut iter = rest.iter();
    while let Some(a) = iter.next() {
        if a == "--path" {
            return iter.next().map(PathBuf::from);
        }
    }
    None
}

/// Whether `githooks run <name>` needs a synthetic push ref list — `name` is
/// either the `pre-push` entrypoint itself, or an individual check whose own
/// declared stage is pre-push. Checked here, rather than left to `.get()` to
/// discover empty, because empty and "nothing to check" are indistinguishable
/// once a check has already started running.
fn needs_synthetic_push_refs(name: &str) -> bool {
    name == "pre-push" || registry::one_named(name).is_some_and(|c| c.stage() == Stage::PrePush)
}
