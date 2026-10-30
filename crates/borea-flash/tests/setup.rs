//! Exercise the user's setup entry point without network, packages or compilers.
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::process::{Command, Output};

use borea_testkit::FakeRoot;

fn fixture() -> FakeRoot {
    let root = FakeRoot::new();
    root.dir("home").dir("release");
    root.file("bin/id", "#!/bin/sh\necho 1000\n");
    root.file("bin/uname", "#!/bin/sh\necho x86_64\n");
    root.file(
        "bin/curl",
        r#"#!/bin/sh
destination=
url=
while [ "$#" -gt 0 ]; do
    case "$1" in
        -o) destination=$2; shift ;;
        https:*) url=$1 ;;
    esac
    shift
done
[ "$BOREA_TEST_DOWNLOAD_FAIL" != 1 ] || exit 22
cp "$BOREA_TEST_RELEASE/${url##*/}" "$destination"
"#,
    );
    for name in ["sudo", "cargo", "rustc", "rustup", "git"] {
        root.file(
            &format!("bin/{name}"),
            "#!/bin/sh\nprintf '%s\\n' \"$0\" >> \"$BOREA_TEST_FORBIDDEN\"\nexit 97\n",
        );
    }
    for entry in fs::read_dir(root.path().join("bin")).unwrap() {
        fs::set_permissions(entry.unwrap().path(), fs::Permissions::from_mode(0o755)).unwrap();
    }
    root.file(
        "release/borea-rataTUI-x86_64-linux",
        "#!/bin/sh\nprintf '%s\\n' \"$*\" > \"$BOREA_TEST_INSTALLED\"\n",
    );
    let digest =
        borea_flash::image::sha256(&root.path().join("release/borea-rataTUI-x86_64-linux"), |_| {})
            .unwrap();
    root.file(
        "release/borea-rataTUI-x86_64-linux.sha256",
        format!("{digest}  borea-rataTUI-x86_64-linux\n"),
    );
    root
}

fn setup(root: &FakeRoot, fail_download: bool) -> Output {
    Command::new("/bin/bash")
        .arg(concat!(env!("CARGO_MANIFEST_DIR"), "/../../scripts/borea-setup.sh"))
        .arg("--no-run")
        .env("HOME", root.path().join("home"))
        .env("PATH", format!("{}:/usr/bin:/bin", root.path().join("bin").display()))
        .env("BOREA_RELEASES_URL", "https://releases.invalid")
        .env("BOREA_TEST_RELEASE", root.path().join("release"))
        .env("BOREA_TEST_DOWNLOAD_FAIL", if fail_download { "1" } else { "0" })
        .env("BOREA_TEST_FORBIDDEN", root.path().join("forbidden"))
        .env("BOREA_TEST_INSTALLED", root.path().join("installed"))
        .output()
        .unwrap()
}

#[test]
fn a_missing_release_never_installs_build_tools_or_clones() {
    let root = fixture();
    let output = setup(&root, true);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("ready-made download is unavailable"));
    assert!(!root.path().join("forbidden").exists());
    assert!(!root.path().join("installed").exists());
}

#[test]
fn a_verified_download_installs_without_a_compiler() {
    let root = fixture();
    let output = setup(&root, false);
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(fs::read_to_string(root.path().join("installed")).unwrap(), "--install\n");
    assert!(!root.path().join("forbidden").exists());
}

#[test]
fn a_bad_checksum_never_executes_the_download_or_falls_back_to_building() {
    let root = fixture();
    root.file("release/borea-rataTUI-x86_64-linux", "this is not the published binary");
    let output = setup(&root, false);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("published SHA-256"));
    assert!(!root.path().join("installed").exists());
    assert!(!root.path().join("forbidden").exists());
}
