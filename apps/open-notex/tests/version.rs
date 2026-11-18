//! The packaged app has to be identifiable without a window, a vault or a GPU:
//! `--version` answers before Rune starts. This runs the real binary, so it also
//! proves the flag is not swallowed by Rune's own command line.

use std::process::Command;

#[test]
fn the_binary_reports_its_version_and_opens_no_window() {
    let name = env!("CARGO_PKG_NAME");
    let bin = env!("CARGO_BIN_EXE_open-notex");

    let output = Command::new(bin)
        .arg("--version")
        .output()
        .expect("the app binary runs");

    assert!(
        output.status.success(),
        "--version failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let line = String::from_utf8_lossy(&output.stdout);
    let line = line.trim();
    assert!(line.starts_with("openNoteX "), "{line}");
    assert!(
        line.contains(env!("CARGO_PKG_VERSION")),
        "the version the crate was built with is in the line: {line}"
    );
    assert!(
        line.ends_with(')'),
        "the build is named so a bug report identifies it: {line}"
    );
    assert!(!line.contains(name), "the crate name is not the app name");
}

#[test]
fn version_in_any_position_still_answers() {
    let output = Command::new(env!("CARGO_BIN_EXE_open-notex"))
        .args(["--snap", "/tmp/never-written.png", "--version"])
        .output()
        .expect("the app binary runs");

    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).starts_with("openNoteX "));
    assert!(
        !std::path::Path::new("/tmp/never-written.png").exists(),
        "asking for the version does not also take a snapshot"
    );
}
