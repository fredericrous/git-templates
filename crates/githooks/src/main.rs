//! The hook binary — argument handling only.
//!
//! Everything it does lives in `githooks-runtime`. Git invokes exactly four
//! hook names; the shim passes its own filename through, and the registry maps
//! that name to a handler.
//!
//! ```text
//! githooks --hooks-dir <dir> <hook-name> [args…]
//! githooks list | install | uninstall [--binary]
//! ```

use std::ffi::OsString;
use std::path::PathBuf;

use githooks_runtime::{pushrefs, registry};

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
        githooks_runtime::list_checks();
        std::process::exit(0);
    }

    // `githooks install` — was a Makefile recipe. It lives here so the guard
    // that decides whether a directory may be emptied has ONE implementation,
    // tested on every platform, rather than one in `make` and another in
    // PowerShell for the Windows users who have no `make` at all.
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
