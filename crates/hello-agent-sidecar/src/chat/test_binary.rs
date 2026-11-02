//! Portable Cargo artifact location test helper. No project or process access.
pub(super) fn sidecar_binary_for_test_executable(
    test_executable: &std::path::Path,
) -> std::path::PathBuf {
    let profile_directory = test_executable
        .parent()
        .and_then(std::path::Path::parent)
        .expect("Cargo test executable is inside a profile deps directory");
    profile_directory.join(format!(
        "hello-agent-sidecar{}",
        std::env::consts::EXE_SUFFIX
    ))
}

#[test]
fn sidecar_binary_follows_cargo_test_profile_directory() {
    for (executable, profile_directory) in [
        (
            "repo/target/debug/deps/hello_agent_sidecar-test",
            "repo/target/debug",
        ),
        (
            "shared/cargo-target/debug/deps/hello_agent_sidecar-test",
            "shared/cargo-target/debug",
        ),
        (
            "shared/cargo-target/aarch64-apple-darwin/release/deps/hello_agent_sidecar-test",
            "shared/cargo-target/aarch64-apple-darwin/release",
        ),
    ] {
        assert_eq!(
            sidecar_binary_for_test_executable(std::path::Path::new(executable)),
            std::path::Path::new(profile_directory).join(format!(
                "hello-agent-sidecar{}",
                std::env::consts::EXE_SUFFIX
            ))
        );
    }
}
