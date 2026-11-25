//! Asset generation commands: `cargo xtask tokens` (T1.2) and `cargo xtask symbols` (T2.3),
//! and `cargo xtask golden [--update]` (T3.2).

use std::process::ExitCode;

mod golden;
mod symbols;
mod tokens;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .as_slice()
    {
        ["tokens"] => tokens::run(&tokens::workspace_root()),
        ["symbols"] => symbols::run(&tokens::workspace_root()),
        ["golden"] => golden::run(&tokens::workspace_root(), false),
        ["golden", "--update"] => golden::run(&tokens::workspace_root(), true),
        _ => Err("usage: cargo xtask <tokens|symbols|golden [--update]>".to_owned()),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("xtask: {error}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{path::Path, process::Command};

    /// INV12: check resolved package names, including renamed and transitive dependencies,
    /// across normal, build, dev, and target-specific edges.
    #[test]
    fn inv12_document_and_symbols_are_gpui_independent() {
        let workspace = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        for package in [
            "omnia-doc",
            "omnia-symbols",
            "omnia-circuit",
            "omnia-agent",
            "omnia-cli",
        ] {
            let output = Command::new(env!("CARGO"))
                .current_dir(workspace)
                .args([
                    "tree",
                    "--locked",
                    "--offline",
                    "--all-features",
                    "--target",
                    "all",
                    "--edges",
                    "normal,build,dev",
                    "--prefix",
                    "none",
                    "--format",
                    "{p}",
                    "-p",
                    package,
                ])
                .output()
                .expect("run cargo tree for INV12");
            assert!(
                output.status.success(),
                "cargo tree failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
            let tree = String::from_utf8(output.stdout).expect("UTF-8 dependency tree");
            for line in tree.lines() {
                let name = line.split_whitespace().next().unwrap_or_default();
                assert!(
                    name != "gpui" && !name.starts_with("gpui_"),
                    "INV12: {package} depends on {line}"
                );
            }
        }
    }
}
