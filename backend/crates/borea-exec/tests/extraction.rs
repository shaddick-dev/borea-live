#![allow(clippy::unwrap_used)]
use std::{fs, os::unix::fs::PermissionsExt, process::Command};
#[test]
fn extraction_is_unprivileged_and_independent_of_service_umask() {
    let directory =
        std::env::temp_dir().join(format!("borea-appimage-extraction-{}", std::process::id()));
    fs::create_dir_all(&directory).unwrap();
    let image = directory.join("fixture.AppImage");
    fs::write(&image, "#!/bin/sh\numask > mask.txt\nmkdir payload\n").unwrap();
    fs::set_permissions(&image, fs::Permissions::from_mode(0o755)).unwrap();
    let status = Command::new("/bin/sh")
        .args([
            "-c",
            "umask 077; exec \"$@\"",
            "borea-fixture",
            env!("CARGO_BIN_EXE_borea-firstboot"),
            "--extract-appimage",
            directory.to_str().unwrap(),
            "fixture.AppImage",
        ])
        .status()
        .unwrap();
    if nix::unistd::Uid::effective().is_root() {
        assert!(!status.success());
        assert!(!directory.join("mask.txt").exists());
    } else {
        assert!(status.success());
        assert_eq!(fs::read_to_string(directory.join("mask.txt")).unwrap().trim(), "0022");
        assert_eq!(
            fs::metadata(directory.join("payload")).unwrap().permissions().mode() & 0o777,
            0o755
        );
    }
    fs::remove_dir_all(directory).unwrap();
}
