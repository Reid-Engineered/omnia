use std::process::Command;

#[test]
fn pinv1_dependency_boundaries() {
    for (package, forbidden) in [
        ("omnia-parts", &[][..]),
        (
            "omnia-doc",
            &["omnia-parts", "omnia-symbols", "toml", "zip", "sha2"][..],
        ),
    ] {
        let output = Command::new(env!("CARGO"))
            .current_dir(env!("CARGO_MANIFEST_DIR"))
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
            .expect("run cargo tree for PINV1");
        assert!(
            output.status.success(),
            "cargo tree failed for {package}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        for name in String::from_utf8(output.stdout)
            .expect("UTF-8 cargo tree output")
            .lines()
            .filter_map(|line| line.split_whitespace().next())
        {
            assert!(
                name != "gpui" && !name.starts_with("gpui_") && !forbidden.contains(&name),
                "{package} depends on {name}"
            );
        }
    }
}
