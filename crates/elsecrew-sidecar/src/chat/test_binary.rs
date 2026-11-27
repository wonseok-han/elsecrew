/// The sidecar executable a test launches, beside the test's own build.
/// Cargo once kept test executables in `<target>/<profile>/deps`; newer Cargo
/// (seen on CI with Rust 1.100) keeps them under
/// `<target>/<profile>/build/<package>/<hash>/out`. The executable is in the
/// nearest folder above the test that holds it; with none on disk (the
/// layout checks below) it is the folder above the test's own.
pub(crate) fn sidecar_binary_for_test_executable(
    test_executable: &std::path::Path,
) -> std::path::PathBuf {
    let name = format!("elsecrew-sidecar{}", std::env::consts::EXE_SUFFIX);
    if let Some(found) = test_executable
        .ancestors()
        .skip(1)
        .map(|folder| folder.join(&name))
        .find(|candidate| candidate.is_file())
    {
        return found;
    }
    let profile_directory = test_executable
        .parent()
        .and_then(std::path::Path::parent)
        .expect("Cargo test executable is inside a profile deps directory");
    profile_directory.join(name)
}

/// The sidecar executable for this test process.
pub(crate) fn test_sidecar_binary() -> std::path::PathBuf {
    sidecar_binary_for_test_executable(&std::env::current_exe().expect("current Cargo test executable"))
}

#[test]
fn sidecar_binary_is_found_in_the_newer_cargo_build_layout() {
    let temp = tempfile::tempdir().unwrap();
    let name = format!("elsecrew-sidecar{}", std::env::consts::EXE_SUFFIX);
    for profile in ["target/debug", "target/aarch64-apple-darwin/release"] {
        let profile = temp.path().join(profile);
        let out = profile.join("build/elsecrew-sidecar/5674a0ca334c9906/out");
        std::fs::create_dir_all(&out).unwrap();
        let test = out.join("elsecrew_sidecar-5674a0ca334c9906");
        std::fs::write(&test, b"").unwrap();
        std::fs::write(profile.join(&name), b"").unwrap();
        assert_eq!(sidecar_binary_for_test_executable(&test), profile.join(&name));
    }
}

#[test]
fn sidecar_binary_follows_cargo_test_profile_directory() {
    for (executable, profile_directory) in [
        (
            "repo/target/debug/deps/elsecrew_sidecar-test",
            "repo/target/debug",
        ),
        (
            "shared/cargo-target/debug/deps/elsecrew_sidecar-test",
            "shared/cargo-target/debug",
        ),
        (
            "shared/cargo-target/aarch64-apple-darwin/release/deps/elsecrew_sidecar-test",
            "shared/cargo-target/aarch64-apple-darwin/release",
        ),
    ] {
        assert_eq!(
            sidecar_binary_for_test_executable(std::path::Path::new(executable)),
            std::path::Path::new(profile_directory).join(format!(
                "elsecrew-sidecar{}",
                std::env::consts::EXE_SUFFIX
            ))
        );
    }
}
