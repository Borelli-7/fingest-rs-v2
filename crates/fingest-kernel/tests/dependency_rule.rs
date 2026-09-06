//! Enforces the hexagonal dependency rule.
//!
//! The kernel and every `*-core` crate are the interior of the hexagon: they may not reference
//! an adapter technology. This scans the declared dependencies of those crates. A transitive
//! leak is only reachable through another pure crate, which this same test also covers.

use std::{fs, path::PathBuf};

const FORBIDDEN: &[&str] = &[
    "sqlx",
    "actix-web",
    "actix-cors",
    "tokio",
    "jsonwebtoken",
    "bcrypt",
    "reqwest",
];

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("crates/<name>/ is two levels below the workspace root")
        .to_path_buf()
}

/// Crate directories that form the interior of the hexagon.
fn pure_crate_manifests() -> Vec<PathBuf> {
    let crates_dir = workspace_root().join("crates");
    let mut manifests: Vec<PathBuf> = fs::read_dir(&crates_dir)
        .expect("crates/ should exist")
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            let name = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or_default();
            name == "fingest-kernel" || name.ends_with("-core")
        })
        .map(|path| path.join("Cargo.toml"))
        .filter(|manifest| manifest.is_file())
        .collect();
    manifests.sort();
    manifests
}

/// Strips comments so a crate name mentioned in a `#` note does not trip the check.
fn declared_dependency_lines(manifest: &str) -> Vec<String> {
    manifest
        .lines()
        .map(|line| line.split('#').next().unwrap_or_default().trim().to_owned())
        .filter(|line| !line.is_empty())
        .collect()
}

#[test]
fn pure_crates_declare_no_adapter_dependencies() {
    let manifests = pure_crate_manifests();
    assert!(
        !manifests.is_empty(),
        "found no pure crates to check — the glob is wrong"
    );

    let mut violations = Vec::new();

    for manifest_path in &manifests {
        let contents = fs::read_to_string(manifest_path)
            .unwrap_or_else(|e| panic!("cannot read {}: {e}", manifest_path.display()));

        for line in declared_dependency_lines(&contents) {
            let Some(key) = line.split(['=', '.', ' ']).next() else {
                continue;
            };
            if FORBIDDEN.contains(&key) {
                violations.push(format!(
                    "{} declares forbidden dependency `{key}`",
                    manifest_path.display()
                ));
            }
        }
    }

    assert!(
        violations.is_empty(),
        "hexagonal dependency rule violated:\n  {}",
        violations.join("\n  ")
    );
}

#[test]
fn kernel_is_among_the_checked_crates() {
    let checked = pure_crate_manifests();
    assert!(
        checked
            .iter()
            .any(|p| p.to_string_lossy().contains("fingest-kernel")),
        "kernel must be covered by the dependency rule, got {checked:?}"
    );
}

#[test]
fn comment_mentions_do_not_trigger_a_violation() {
    let lines = declared_dependency_lines("# sqlx is banned here\nserde = \"1\"\n");
    assert_eq!(lines, vec!["serde = \"1\""]);
}
