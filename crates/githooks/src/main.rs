//! The hook binary — argument handling only.
//!
//! Everything it does lives in `githooks-runtime`. Git invokes exactly four
//! hook names; the shim passes its own filename through, and the registry maps
//! that name to a handler.
//!
//! ```text
//! githooks --hooks-dir <dir> <hook-name> [args…]
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
    let code = match registry::lookup(&hook) {
        Some(f) => f(&ctx),
        None => {
            eprintln!("githooks: unknown hook {hook:?}");
            2
        }
    };
    std::process::exit(code);
}
