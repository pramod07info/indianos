#![cfg(all(feature = "desktop-volume", feature = "desktop-fixture"))]
//! Admit the actual kernel-generated COW fixture, then reject damaged media.
use std::{fs, process::Command};

#[test]
fn existing_gpt_selection_is_read_only_healthy_and_never_overwrites_output() {
    let dir = std::env::temp_dir().join(format!("indianos-desktop-pin-{}", std::process::id()));
    fs::create_dir(&dir).unwrap();
    let raw = dir.join("volume.raw");
    let expected = dir.join("expected.bin");
    let pinned = dir.join("pinned.bin");
    assert!(Command::new(env!("CARGO_BIN_EXE_indianos-desktop-fixture"))
        .args([
            "--output",
            raw.to_str().unwrap(),
            "--selection-output",
            expected.to_str().unwrap()
        ])
        .status()
        .unwrap()
        .success());
    let before = fs::read(&raw).unwrap();
    let pin = |ordinal: &str, output: &std::path::Path| {
        Command::new(env!("CARGO_BIN_EXE_indianos-desktop-volume"))
            .args([
                "--image",
                raw.to_str().unwrap(),
                "--partition",
                ordinal,
                "--output",
                output.to_str().unwrap(),
            ])
            .output()
            .unwrap()
    };
    let result = pin("0", &pinned);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert_eq!(fs::read(&pinned).unwrap(), fs::read(&expected).unwrap());
    assert_eq!(fs::read(&raw).unwrap(), before);
    assert!(!pin("0", &pinned).status.success());
    assert_eq!(fs::read(&pinned).unwrap(), fs::read(&expected).unwrap());
    let rejected = dir.join("rejected.bin");
    assert!(!pin("1", &rejected).status.success());
    assert!(!rejected.exists());
    let mut corrupt = before.clone();
    corrupt[512 + 16] ^= 1;
    fs::write(&raw, &corrupt).unwrap();
    assert!(!pin("0", &rejected).status.success());
    assert!(!rejected.exists());
    assert_eq!(fs::read(&raw).unwrap(), corrupt);
    let mut damaged = before;
    damaged[40 * 512..140 * 512].fill(0);
    fs::write(&raw, &damaged).unwrap();
    assert!(!pin("0", &rejected).status.success());
    assert!(!rejected.exists());
    assert_eq!(fs::read(&raw).unwrap(), damaged);
    let check = |path: &std::path::Path| {
        Command::new(env!("CARGO_BIN_EXE_indianos-desktop-volume"))
            .args(["--check-selection", path.to_str().unwrap()])
            .output()
            .unwrap()
    };
    assert!(check(&pinned).status.success());
    fs::write(&rejected, [0; 65]).unwrap();
    assert!(!check(&rejected).status.success());
    fs::remove_dir_all(dir).unwrap();
}
