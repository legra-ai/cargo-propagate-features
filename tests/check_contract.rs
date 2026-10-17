//! Nonmutating CI checks and dependency feature semantics.

use std::env;
use std::path::Path;

use async_fs_io::{
    TempDir,
    ensure_dir,
    read_string_bounded,
    write_bytes,
};
use tokio::process::Command;

async fn fixture(dependency: &str, feature: &str) -> TempDir {
    let directory = TempDir::create(env::temp_dir())
        .await
        .expect("workspace fixture");
    write_bytes(
        directory.path().join("Cargo.toml"),
        b"[workspace]\nmembers=['a','b']\nresolver='3'\n[workspace.dependencies]\nb={path='b'}\n",
    )
    .await
    .expect("workspace manifest");
    for name in ["a", "b"] {
        ensure_dir(directory.path().join(name).join("src"))
            .await
            .expect("member source");
        write_bytes(
            directory.path().join(name).join("src/lib.rs"),
            b"//! Fixture.\n",
        )
        .await
        .expect("source");
    }
    write_bytes(directory.path().join("a/Cargo.toml"), format!(
        "[package]\nname='a'\nversion='0.1.0'\nedition='2024'\n[dependencies]\n{dependency}\n[features]\ndefault=[]\nweb=[{feature}]\n").as_bytes())
        .await.expect("dependent manifest");
    write_bytes(
        directory.path().join("b/Cargo.toml"),
        b"[package]\nname='b'\nversion='0.1.0'\nedition='2024'\n[features]\ndefault=[]\nweb=[]\n",
    )
    .await
    .expect("dependency manifest");
    directory
}

fn invoke(root: &Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_cargo-propagate-features"));
    command
        .args([
            "propagate-features",
            "--features",
            "web",
            "--quiet",
            "--manifest-path",
        ])
        .arg(root.join("Cargo.toml"));
    command
}

#[tokio::test]
async fn check_detects_drift_without_modifying_manifests_then_passes_after_fix() {
    let directory = fixture("b={path='../b'}", "").await;
    let manifest = directory.path().join("a/Cargo.toml");
    let before = read_string_bounded(&manifest, 65536)
        .await
        .expect("manifest");
    let result = invoke(directory.path())
        .arg("--check")
        .output()
        .await
        .expect("check");
    assert_eq!(
        result.status.code(),
        Some(1),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert_eq!(
        read_string_bounded(&manifest, 65536)
            .await
            .expect("manifest"),
        before
    );
    assert!(
        invoke(directory.path())
            .output()
            .await
            .expect("apply")
            .status
            .success()
    );
    assert!(
        invoke(directory.path())
            .arg("--check")
            .output()
            .await
            .expect("clean check")
            .status
            .success()
    );
}

#[tokio::test]
async fn cleanup_only_drift_also_fails_check_without_writing() {
    let directory = fixture("b={workspace=true,features=['web']}", "'b/web'").await;
    let manifest = directory.path().join("a/Cargo.toml");
    let before = read_string_bounded(&manifest, 65536)
        .await
        .expect("manifest");
    let result = invoke(directory.path())
        .arg("--check")
        .output()
        .await
        .expect("check");
    assert_eq!(
        result.status.code(),
        Some(1),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert_eq!(
        read_string_bounded(&manifest, 65536)
            .await
            .expect("manifest"),
        before
    );
    assert!(
        invoke(directory.path())
            .output()
            .await
            .expect("apply")
            .status
            .success()
    );
    assert!(
        invoke(directory.path())
            .arg("--check")
            .output()
            .await
            .expect("clean check")
            .status
            .success()
    );
}

#[tokio::test]
async fn renamed_optional_dependency_uses_weak_feature_forwarding() {
    let directory = fixture("renamed={package='b',path='../b',optional=true}", "").await;
    let result = invoke(directory.path()).output().await.expect("apply");
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let manifest = read_string_bounded(directory.path().join("a/Cargo.toml"), 65536)
        .await
        .expect("manifest")
        .parse::<toml_edit::DocumentMut>()
        .expect("valid manifest");
    let values = manifest["features"]["web"]
        .as_array()
        .expect("feature list");
    assert_eq!(values.len(), 1);
    assert_eq!(
        values.get(0).and_then(toml_edit::Value::as_str),
        Some("renamed?/web")
    );
    assert!(
        invoke(directory.path())
            .arg("--check")
            .output()
            .await
            .expect("valid clean graph")
            .status
            .success()
    );
}

#[tokio::test]
async fn installed_version_is_embedded_package_identity() {
    let result = Command::new(env!("CARGO_BIN_EXE_cargo-propagate-features"))
        .args(["propagate-features", "--version"])
        .output()
        .await
        .expect("version");
    assert!(result.status.success());
    assert_eq!(
        String::from_utf8(result.stdout).expect("version text"),
        format!(
            "cargo-propagate-features {}.{}.{}\n",
            env!("CARGO_PKG_VERSION_MAJOR"),
            env!("CARGO_PKG_VERSION_MINOR"),
            env!("CARGO_PKG_VERSION_PATCH")
        )
    );
}
