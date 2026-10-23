//! Buildroot's post-build and post-image hooks.
//!
//! Buildroot runs these with its usual environment (`HOST_DIR`, `TARGET_DIR`,
//! `BINARIES_DIR`, `BR2_EXTERNAL_BOREA_PATH`) plus what `borea-build iso` exported:
//! `BOREA_LIVE_VERSION`, `BOREA_LIVE_BUILD_ID`, and for signed builds
//! `BOREA_SB_KEY` and `BOREA_SB_CERT`.

use std::fs;
use std::path::{Path, PathBuf};

use crate::Result;
use crate::initramfs::Image;
use crate::run::{arg, blake2b, sha256, tool};

fn env(name: &str) -> Result<String> {
    std::env::var(name)
        .map_err(|_| format!("{name} is not set; run this through `borea-build iso`"))
}

fn write(path: &Path, contents: impl AsRef<[u8]>) -> Result {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
    }
    fs::write(path, contents).map_err(|e| format!("{}: {e}", path.display()))
}

fn copy(from: &Path, to: &Path) -> Result {
    if let Some(parent) = to.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
    }
    fs::copy(from, to).map(drop).map_err(|e| format!("{} → {}: {e}", from.display(), to.display()))
}

/// The finished root filesystem, before it is packed.
/// Files the live session cannot work without, though nothing fails at build
/// time when they are missing: each was once missing from a build that booted.
const SESSION_NEEDS: &[(&str, &str)] = &[
    ("usr/share/X11/xkb/rules/evdev", "keymaps (xkeyboard-config): cage would have no keyboard"),
    ("usr/bin/cage", "the kiosk compositor"),
    ("usr/bin/borea-live", "the installer"),
    ("usr/bin/borea-worker", "the privileged installer"),
    ("usr/bin/mkfs.fat", "dosfstools' mkfs.fat: EFI partitions could not be formatted"),
    ("usr/bin/sfdisk", "util-linux's sfdisk: disks could not be partitioned"),
    ("usr/bin/unshare", "util-linux's unshare: the worker needs a private mount namespace"),
    ("usr/bin/chroot", "chroot: Arch's bootstrap tools run through it"),
    ("etc/pam.d/borea-live", "the installer's PAM session: no XDG_RUNTIME_DIR without it"),
    (
        "etc/systemd/system/sysinit.target.wants/sys-firmware-efi-efivars.mount",
        "the UEFI variables mount: Secure Boot state would be unknown",
    ),
];

/// Programs that must be built against a library: Buildroot can quietly build
/// them without it.
const LINKED: &[(&str, &str, &str)] = &[(
    "usr/lib/polkit-1/polkitd",
    "libsystemd.so",
    "polkit without logind sees no session as local or active, so the live user's rule never applies",
)];

/// What the session needs and `target` lacks.
fn missing(target: &Path) -> Vec<String> {
    let files = SESSION_NEEDS
        .iter()
        .filter(|(path, _)| fs::symlink_metadata(target.join(path)).is_err())
        .map(|(path, why)| format!("/{path}: {why}"));
    let links = LINKED
        .iter()
        .filter(|(path, library, _)| {
            !fs::read(target.join(path))
                .is_ok_and(|bytes| bytes.windows(library.len()).any(|w| w == library.as_bytes()))
        })
        .map(|(path, library, why)| format!("/{path} is not linked with {library}: {why}"));
    files.chain(links).collect()
}

pub fn post_build(target: &Path) -> Result {
    let missing = missing(target);
    if !missing.is_empty() {
        return Err(format!("the live session would be broken:\n  {}", missing.join("\n  ")));
    }
    // The marker that tells Borea it runs in its own reference environment.
    write(&target.join("etc/borea-live"), format!("{}\n", env("BOREA_LIVE_VERSION")?))?;
    // Identifies this build, so the early boot finds its own medium and no other.
    write(&target.join("etc/borea-live-build"), format!("{}\n", env("BOREA_LIVE_BUILD_ID")?))
}

struct Signing {
    key: String,
    cert: String,
}

/// Turn the kernel and the EROFS root into a hybrid BIOS/UEFI ISO booted by Limine.
pub fn post_image(images: &Path) -> Result {
    let host = PathBuf::from(env("HOST_DIR")?);
    let target = PathBuf::from(env("TARGET_DIR")?);
    let board = PathBuf::from(env("BR2_EXTERNAL_BOREA_PATH")?).join("board/borea");
    let build_id = env("BOREA_LIVE_BUILD_ID")?;
    let signing = match (std::env::var("BOREA_SB_KEY"), std::env::var("BOREA_SB_CERT")) {
        (Ok(key), Ok(cert)) => Some(Signing { key, cert }),
        _ => None,
    };

    // ── Early boot ──────────────────────────────────────────────────────────
    let mut early = Image::new();
    early.program(&target, "/usr/lib/borea/borea-init", "/init")?;
    early.file("/build-id", 0o644, format!("{build_id}\n").into_bytes());
    match &signing {
        // Signed builds check the system image before trusting it, and carry no
        // shell that could bypass that check.
        Some(_) => early.file(
            "/rootfs.sha256",
            0o644,
            format!("{}\n", sha256(&images.join("rootfs.erofs"))?).into_bytes(),
        ),
        // Unsigned builds keep busybox's shell for rescue.
        None => early.program(&target, "/bin/busybox", "/bin/busybox")?,
    }
    write(&images.join("initramfs.img"), early.pack()?)?;

    // ── The ISO tree ────────────────────────────────────────────────────────
    let root = images.join("iso-root");
    if root.exists() {
        fs::remove_dir_all(&root).map_err(|e| e.to_string())?;
    }
    let limine = host.join("share/limine");
    copy(&images.join("bzImage"), &root.join("boot/vmlinuz"))?;
    copy(&images.join("initramfs.img"), &root.join("boot/initramfs.img"))?;
    copy(&images.join("rootfs.erofs"), &root.join("live/rootfs.erofs"))?;
    write(&root.join("live/build-id"), format!("{build_id}\n"))?;
    for file in ["limine-bios.sys", "limine-bios-cd.bin", "limine-uefi-cd.bin"] {
        copy(&limine.join(file), &root.join("boot/limine").join(file))?;
    }
    copy(&limine.join("BOOTX64.EFI"), &root.join("EFI/BOOT/BOOTX64.EFI"))?;

    let config = fs::read_to_string(board.join("limine.conf")).map_err(|e| e.to_string())?;
    match &signing {
        Some(signing) => sign(&host, &root, &config, signing)?,
        None => write(&root.join("boot/limine/limine.conf"), config)?,
    }

    let iso = images.join("borea-live.iso");
    tool(
        arg(&host.join("bin/xorriso"))?,
        &[
            "-as",
            "mkisofs",
            "-iso-level",
            "3",
            "-V",
            "BOREA_LIVE",
            "-R",
            "-r",
            "-J",
            "-b",
            "boot/limine/limine-bios-cd.bin",
            "-no-emul-boot",
            "-boot-load-size",
            "4",
            "-boot-info-table",
            "-hfsplus",
            "-apm-block-size",
            "2048",
            "--efi-boot",
            "boot/limine/limine-uefi-cd.bin",
            "-efi-boot-part",
            "--efi-boot-image",
            "--protective-msdos-label",
            arg(&root)?,
            "-o",
            arg(&iso)?,
        ],
    )?;
    tool(arg(&host.join("bin/limine"))?, &["bios-install", arg(&iso)?])
}

/// Secure Boot: every file Limine loads carries its BLAKE2b hash, the config's own
/// hash is baked into Limine, and Limine is signed. Under Secure Boot, Limine then
/// refuses anything that does not match, and its config editor stays off.
fn sign(host: &Path, root: &Path, config: &str, signing: &Signing) -> Result {
    let config = hash_paths(
        config,
        &[
            ("boot():/boot/vmlinuz", blake2b(&root.join("boot/vmlinuz"))?),
            ("boot():/boot/initramfs.img", blake2b(&root.join("boot/initramfs.img"))?),
        ],
    );
    let conf = root.join("boot/limine/limine.conf");
    write(&conf, &config)?;
    let efi = root.join("EFI/BOOT/BOOTX64.EFI");
    tool(arg(&host.join("bin/limine"))?, &["enroll-config", arg(&efi)?, &blake2b(&conf)?])?;
    tool(
        "sbsign",
        &["--key", &signing.key, "--cert", &signing.cert, "--output", arg(&efi)?, arg(&efi)?],
    )?;

    // Firmware booting the ISO as a CD reads Limine from this FAT image, so it
    // must hold the signed copy too.
    let fat = root.join("boot/limine/limine-uefi-cd.bin");
    fs::remove_file(&fat).ok();
    fs::File::create(&fat).and_then(|f| f.set_len(8 << 20)).map_err(|e| e.to_string())?;
    let mtools = |program: &str, args: &[&str]| tool(arg(&host.join("bin").join(program))?, args);
    mtools("mformat", &["-i", arg(&fat)?, "-v", "BOREA_EFI", "::"])?;
    mtools("mmd", &["-i", arg(&fat)?, "::/EFI", "::/EFI/BOOT"])?;
    mtools("mcopy", &["-i", arg(&fat)?, arg(&efi)?, "::/EFI/BOOT/BOOTX64.EFI"])?;
    copy(Path::new(&signing.cert), &root.join("boot/limine/borea-live-db.crt"))
}

/// Append `#<hash>` to every line that ends in one of the given paths.
fn hash_paths(config: &str, hashes: &[(&str, String)]) -> String {
    config
        .lines()
        .map(|line| {
            hashes
                .iter()
                .find(|(path, _)| line.trim_end().ends_with(path))
                .map_or_else(|| line.to_owned(), |(_, hash)| format!("{}#{hash}", line.trim_end()))
        })
        .collect::<Vec<_>>()
        .join("\n")
        + "\n"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_loaded_file_gets_its_hash() {
        let config = "/Borea Live\n    path: boot():/boot/vmlinuz\n    module_path: boot():/boot/initramfs.img\n    cmdline: quiet\n";
        let hashed = hash_paths(
            config,
            &[("boot():/boot/vmlinuz", "aa".into()), ("boot():/boot/initramfs.img", "bb".into())],
        );
        assert!(hashed.contains("path: boot():/boot/vmlinuz#aa\n"));
        assert!(hashed.contains("module_path: boot():/boot/initramfs.img#bb\n"));
        assert!(hashed.contains("cmdline: quiet\n"), "other lines stay as they were");
    }

    #[test]
    fn a_session_without_keymaps_is_refused() {
        let target = std::env::temp_dir().join(format!("borea-hooks-{}", std::process::id()));
        let _ = fs::remove_dir_all(&target);
        for (path, _) in SESSION_NEEDS {
            fs::create_dir_all(target.join(path).parent().unwrap()).unwrap();
            fs::write(target.join(path), "").unwrap();
        }
        for (path, library, _) in LINKED {
            fs::create_dir_all(target.join(path).parent().unwrap()).unwrap();
            fs::write(target.join(path), format!("ELF…{library}.0…")).unwrap();
        }
        assert!(missing(&target).is_empty());
        fs::remove_file(target.join("usr/share/X11/xkb/rules/evdev")).unwrap();
        let missing = missing(&target);
        assert_eq!(missing.len(), 1);
        assert!(missing[0].contains("keyboard"));
        fs::write(target.join(LINKED[0].0), "ELF without it").unwrap();
        assert_eq!(super::missing(&target).len(), 2);
        fs::remove_dir_all(&target).unwrap();
    }

    #[test]
    fn the_overlay_provides_what_it_should() {
        let overlay = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../live/buildroot/board/borea/rootfs-overlay");
        for path in SESSION_NEEDS.iter().map(|(p, _)| p).filter(|p| p.starts_with("etc/")) {
            assert!(fs::symlink_metadata(overlay.join(path)).is_ok(), "overlay lacks /{path}");
        }
    }
}
