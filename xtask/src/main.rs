//! Asset generation commands; implemented by subsequent asset tasks.

fn main() {}

#[cfg(test)]
mod tests {
    use std::{path::Path, process::Command};

    /// INV12: check resolved package names, including renamed and transitive dependencies,
    /// across normal, build, dev, and target-specific edges.
    #[test]
    fn inv12_document_and_symbols_are_gpui_independent() {
        let workspace = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        for package in ["omnia-doc", "omnia-symbols"] {
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
