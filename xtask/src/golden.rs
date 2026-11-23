//! `cargo xtask golden [--update]` (T3.2): runs every golden-image suite, or rewrites their
//! PNGs. The suites are `harness = false` test targets built on the `omnia-golden` harness
//! (`tests/golden/`); `--update` sets `OMNIA_GOLDEN_UPDATE=1` for them. Review the rewritten
//! PNGs before committing them.

use std::{path::Path, process::Command};

/// Set to `1` to rewrite goldens; must match `omnia_golden::UPDATE_ENV`.
const UPDATE_ENV: &str = "OMNIA_GOLDEN_UPDATE";

/// Every golden suite as (package, test target). A suite added elsewhere (U1.9's shell
/// goldens) adds its line here.
pub const SUITES: &[(&str, &str)] = &[("omnia-golden", "sheet"), ("omnia-golden", "overlay")];

/// The command for one suite.
fn command(cargo: &std::ffi::OsStr, root: &Path, suite: (&str, &str), update: bool) -> Command {
    let (package, test) = suite;
    let mut command = Command::new(cargo);
    command
        .current_dir(root)
        .args(["test", "--package", package, "--test", test]);
    if update {
        command.env(UPDATE_ENV, "1");
    } else {
        command.env_remove(UPDATE_ENV);
    }
    command
}

pub fn run(root: &Path, update: bool) -> Result<(), String> {
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let mut failed = Vec::new();
    for &suite in SUITES {
        let status = command(&cargo, root, suite, update)
            .status()
            .map_err(|e| format!("run cargo test for {}: {e}", suite.1))?;
        if !status.success() {
            failed.push(format!("{}/{}", suite.0, suite.1));
        }
    }
    if failed.is_empty() {
        if update {
            println!(
                "golden: rewrote the PNGs of {} suite(s); review them",
                SUITES.len()
            );
        }
        Ok(())
    } else {
        Err(format!("golden suites failed: {}", failed.join(", ")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn suites_exist_as_test_targets() {
        let root = crate::tokens::workspace_root();
        for (package, test) in SUITES {
            let dir = match *package {
                "omnia-golden" => root.join("tests/golden"),
                other => root.join("crates").join(other),
            };
            assert!(
                dir.join("tests").join(format!("{test}.rs")).exists(),
                "{package} has no test target {test}"
            );
        }
    }

    #[test]
    fn update_sets_the_variable_and_compare_clears_it() {
        let root = Path::new(".");
        let env = |update| {
            command("cargo".as_ref(), root, SUITES[0], update)
                .get_envs()
                .find(|(k, _)| *k == UPDATE_ENV)
                .map(|(_, v)| v.map(|v| v.to_owned()))
        };
        assert_eq!(env(true), Some(Some("1".into())));
        assert_eq!(
            env(false),
            Some(None),
            "removed, so a stray variable cannot update"
        );
    }
}
