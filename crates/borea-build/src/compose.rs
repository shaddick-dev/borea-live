//! Assemble an independent Borea live host from authenticated prebuilt packages.
//! Only Borea's Rust programs and Limine's small host utility are compiled here.
use crate::run::{arg, command, output, sha256, tool};
use crate::{Result, hooks, iso};
use std::fs;
use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::{Path, PathBuf};

const LIMINE_URL: &str =
    "https://github.com/Limine-Bootloader/Limine/releases/download/v12.9.4/limine-binary.tar.xz";
const LIMINE_SHA: &str = "9880cc04681262214db9b824ed8a5c55296955027d8caa9f55ab52a6dd85d6a0";
const BINS: &[&str] = &[
    "borea-live",
    "borea",
    "borea-flash",
    "borea-installd",
    "borea-cli",
    "borea-init",
    "borea-session",
    "borea-build",
];
const MODULES: &[&str] = &[
    "erofs",
    "overlay",
    "isofs",
    "loop",
    "sd_mod",
    "sr_mod",
    "usb_storage",
    "uas",
    "xhci_pci",
    "ehci_pci",
    "ohci_pci",
    "uhci_hcd",
    "nvme",
    "virtio_pci",
    "virtio_blk",
    "virtio_scsi",
    "vmd",
    "ahci",
    "ata_piix",
];

/// USB network adapters Borea Live must drive out of the box: a desktop with no
/// internal Wi-Fi gets online with whatever adapter (or phone) is to hand.
/// Checked at build time, so a release can never ship without them.
const USB_NETWORK_MODULES: &[&str] = &[
    // Realtek, the most common cheap adapters.
    "rtl8xxxu",
    "rtl8192cu",
    "rtl8187",
    "rtw88_8821cu",
    "rtw88_8822bu",
    "rtw88_8822cu",
    "rtw88_8723du",
    "rtw88_8812au",
    "rtw88_8821au",
    "rtw89_8851bu",
    "rtw89_8852bu",
    // MediaTek and Ralink.
    "mt7601u",
    "mt76x0u",
    "mt76x2u",
    "mt7921u",
    "mt7925u",
    "rt2800usb",
    "rt73usb",
    // Atheros, Broadcom, ZyDAS, Marvell.
    "ath9k_htc",
    "carl9170",
    "ar5523",
    "brcmfmac",
    "zd1211rw",
    "usb8xxx",
    // A phone's USB tethering (Android, iPhone) and USB Ethernet.
    "rndis_host",
    "cdc_ether",
    "cdc_ncm",
    "ipheth",
    "r8152",
    "ax88179_178a",
];
/// Their firmware, as directories or files under /lib/firmware (any compression).
const USB_NETWORK_FIRMWARE: &[&str] = &[
    "rtw88",
    "rtw89",
    "rtlwifi",
    "mediatek",
    "ath9k_htc",
    "brcm",
    "mt7601u.bin",
    "rt2870.bin",
    "carl9170-1.fw",
];

fn check_usb_network(root: &Path, kernel: &str) -> Result {
    let mut missing = Vec::new();
    for name in USB_NETWORK_MODULES {
        let found = command("modprobe")
            .args(["--dirname", arg(root)?, "--show-depends", "--set-version", kernel, name])
            .output()
            .map_err(|e| e.to_string())?
            .status
            .success();
        if !found {
            missing.push(format!("module {name}"));
        }
    }
    let firmware = root.join("usr/lib/firmware");
    for name in USB_NETWORK_FIRMWARE {
        let found =
            ["", ".xz", ".zst"].iter().any(|ext| firmware.join(format!("{name}{ext}")).exists());
        if !found {
            missing.push(format!("firmware {name}"));
        }
    }
    if missing.is_empty() {
        return Ok(());
    }
    Err(format!(
        "Borea Live must get online with USB Wi-Fi adapters and phone tethering; missing: {}",
        missing.join(", ")
    ))
}

struct Container(String, signal_hook::iterator::Handle);
impl Container {
    fn new(name: String) -> Result<Self> {
        use signal_hook::{
            consts::{SIGINT, SIGTERM},
            iterator::Signals,
        };
        let mut signals = Signals::new([SIGINT, SIGTERM]).map_err(|e| e.to_string())?;
        let handle = signals.handle();
        let cleanup = name.clone();
        std::thread::spawn(move || {
            if signals.forever().next().is_some() {
                let _ = command("docker").args(["rm", "-f", &cleanup]).output();
                std::process::exit(130);
            }
        });
        Ok(Self(name, handle))
    }
}
impl Drop for Container {
    fn drop(&mut self) {
        self.1.close();
        let _ = command("docker").args(["rm", "-f", &self.0]).output();
    }
}
fn write(path: impl AsRef<Path>, contents: impl AsRef<[u8]>) -> Result {
    let path = path.as_ref();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    fs::write(path, contents).map_err(|e| format!("{}: {e}", path.display()))?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o644)).map_err(|e| e.to_string())
}
fn link(to: &str, at: &str) -> Result {
    let _ = fs::remove_file(at);
    if let Some(parent) = Path::new(at).parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    symlink(to, at).map_err(|e| format!("{at}: {e}"))
}

fn stage_root(at: &Path) -> Result {
    fs::create_dir_all(at).map_err(|e| e.to_string())?;
    fs::set_permissions(at, fs::Permissions::from_mode(0o755)).map_err(|e| e.to_string())?;
    for entry in fs::read_dir("/").map_err(|e| e.to_string())?.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if [
            "src",
            "work",
            "cargo",
            "rust",
            "borea-bins",
            "out",
            "proc",
            "sys",
            "dev",
            "run",
            "tmp",
            ".dockerenv",
            "etc",
        ]
        .contains(&name.as_str())
        {
            continue;
        }
        tool("cp", &["-al", arg(&entry.path())?, arg(at)?])?;
    }
    for name in ["proc", "sys", "dev", "run", "tmp", "etc"] {
        fs::create_dir_all(at.join(name)).map_err(|e| e.to_string())?;
        fs::set_permissions(at.join(name), fs::Permissions::from_mode(0o755))
            .map_err(|e| e.to_string())?;
    }
    for entry in fs::read_dir("/etc").map_err(|e| e.to_string())?.flatten() {
        if ["hosts", "hostname", "resolv.conf"].iter().any(|s| entry.file_name() == *s) {
            continue;
        }
        tool("cp", &["-al", arg(&entry.path())?, arg(&at.join("etc"))?])?;
    }
    Ok(())
}

/// A private Git checkout must not make the live /etc unreadable to dbus.
fn overlay(from: &Path, to: &Path) -> Result {
    fs::create_dir_all(to).map_err(|e| e.to_string())?;
    fs::set_permissions(to, fs::Permissions::from_mode(0o755)).map_err(|e| e.to_string())?;
    for entry in fs::read_dir(from).map_err(|e| e.to_string())?.flatten() {
        let at = to.join(entry.file_name());
        let kind = entry.file_type().map_err(|e| e.to_string())?;
        if kind.is_symlink() {
            let _ = fs::remove_file(&at);
            symlink(fs::read_link(entry.path()).map_err(|e| e.to_string())?, at)
                .map_err(|e| e.to_string())?;
        } else if kind.is_dir() {
            overlay(&entry.path(), &at)?;
        } else if kind.is_file() {
            copy(&entry.path(), &at)?;
            fs::set_permissions(at, fs::Permissions::from_mode(0o644))
                .map_err(|e| e.to_string())?;
        } else {
            return Err("the live overlay contains an unsupported special file".into());
        }
    }
    Ok(())
}

fn root_link(root: &Path, to: &str, at: &str) -> Result {
    link(to, arg(&root.join(at))?)
}
fn copy(from: &Path, to: &Path) -> Result {
    if let Some(parent) = to.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        fs::set_permissions(parent, fs::Permissions::from_mode(0o755))
            .map_err(|e| e.to_string())?;
    }
    fs::copy(from, to).map(drop).map_err(|e| format!("{}: {e}", from.display()))
}
fn exec(container: &Container, uid: &str, args: &[&str]) -> Result {
    let mut cmd = vec!["exec", "--user", uid, "--workdir", "/src", &container.0];
    cmd.extend(args);
    tool("docker", &cmd)
}

pub fn run(args: &[&str]) -> Result {
    if std::env::consts::OS != "linux" || std::env::consts::ARCH != "x86_64" {
        return Err("composition currently needs an x86-64 Linux build host".into());
    }
    let mut refresh = false;
    let mut ca = None;
    let mut bins = None;
    let mut args = args.iter();
    while let Some(option) = args.next() {
        match *option {
            "--refresh-runtime" => refresh = true,
            "--ca-cert" => ca = Some(PathBuf::from(args.next().ok_or("--ca-cert needs a path")?)),
            "--bin-dir" => bins = Some(PathBuf::from(args.next().ok_or("--bin-dir needs a path")?)),
            other => return Err(format!("unknown composition option {other}")),
        }
    }
    if rustix::process::geteuid().is_root() {
        return Err("run composition as your normal user with Docker access".into());
    }
    if let Some(bins) = &bins {
        for name in BINS.iter().copied().chain(["borea-firstboot"]) {
            if !bins.join(name).is_file() {
                return Err(format!("{} is missing {name}", bins.display()));
            }
        }
    }
    let repo = crate::repo()?;
    let work = repo.join("target/live");
    fs::create_dir_all(work.join("out")).map_err(|e| e.to_string())?;
    let _lock = iso::lock(&work)?;
    tool("docker", &["info", "--format", "{{.OSType}}"])
        .map_err(|e| format!("Composition needs Docker with Linux container access. {e}"))?;
    let runtime =
        format!("borea-compose-runtime:{}", &sha256(&repo.join("live/compose/Dockerfile"))?[..16]);
    println!(">>> compose 1 Downloading prebuilt kernel, Mesa, drivers and live tools (cached)");
    let mut build = command("docker");
    build.args(["build", "--platform", "linux/amd64", "--progress=plain", "-t", &runtime]);
    if refresh {
        build.args(["--pull", "--no-cache"]);
    }
    if let Some(ca) = &ca {
        build.arg("--secret").arg(format!(
            "id=proxy_ca,src={}",
            ca.canonicalize().map_err(|e| e.to_string())?.display()
        ));
    }
    if !build.arg(repo.join("live/compose")).status().map_err(|e| e.to_string())?.success() {
        return Err(
            "could not download the composition runtime; no source build was started".into()
        );
    }
    let archive = work.join("compose-limine.tar.xz");
    if !archive.is_file() || sha256(&archive)? != LIMINE_SHA {
        let pending = archive.with_extension("part");
        tool("curl", &["--fail", "--location", "--output", arg(&pending)?, LIMINE_URL])?;
        if sha256(&pending)? != LIMINE_SHA {
            let _ = fs::remove_file(pending);
            return Err("the prebuilt Limine download failed its pinned SHA-256 check".into());
        }
        fs::rename(pending, &archive).map_err(|e| e.to_string())?;
    }
    let cargo_home = std::env::var_os("CARGO_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cargo")))
        .ok_or("CARGO_HOME or HOME must be set")?;
    let sysroot = PathBuf::from(output("rustc", &["--print", "sysroot"])?.trim());
    let cache = work.join("compose");
    fs::create_dir_all(&cache).map_err(|e| e.to_string())?;
    let id = iso::build_id()?;
    let name = format!("borea-compose-{}", &id[..12]);
    let container = Container::new(name.clone())?;
    let mut create = command("docker");
    create.args(["create", "--platform", "linux/amd64", "--name", &name]);
    for (source, dest, readonly) in [
        (repo.clone(), "/src", true),
        (cache, "/work", false),
        (cargo_home, "/cargo", false),
        (sysroot, "/rust", true),
    ] {
        create.arg("--mount").arg(format!(
            "type=bind,src={},dst={dest}{}",
            source.display(),
            if readonly { ",readonly" } else { "" }
        ));
    }
    if let Some(bins) = &bins {
        create.arg("--mount").arg(format!(
            "type=bind,src={},dst=/borea-bins,readonly",
            bins.canonicalize().map_err(|e| e.to_string())?.display()
        ));
    }
    if let Some(ca) = ca {
        create.arg("--mount").arg(format!(
            "type=bind,src={},dst=/run/borea-build-ca.pem,readonly",
            ca.canonicalize().map_err(|e| e.to_string())?.display()
        ));
        create.args(["--env", "CARGO_HTTP_CAINFO=/run/borea-build-ca.pem"]);
    }
    create.args([
        "--env",
        "CARGO_HOME=/cargo",
        "--env",
        "RUSTC=/rust/bin/rustc",
        "--env",
        "PATH=/rust/bin:/usr/sbin:/usr/bin:/sbin:/bin",
    ]);
    let jobs = std::env::var("CARGO_BUILD_JOBS").unwrap_or_else(|_| {
        std::thread::available_parallelism().map_or(2, |n| n.get().min(4)).to_string()
    });
    create.arg("--env").arg(format!("CARGO_BUILD_JOBS={jobs}"));
    create.args([&runtime, "sleep", "infinity"]);
    if !create.status().map_err(|e| e.to_string())?.success() {
        return Err("could not create the composition container".into());
    }
    tool("docker", &["start", &container.0])?;
    let uid =
        format!("{}:{}", rustix::process::geteuid().as_raw(), rustix::process::getegid().as_raw());
    if bins.is_none() {
        println!(">>> compose 1 Building only Borea's Rust programs (dependencies cached)");
        exec(
            &container,
            &uid,
            &[
                "/rust/bin/cargo",
                "build",
                "--locked",
                "--profile",
                "app",
                "--target-dir",
                "/work/cargo",
                "-p",
                "borea-gui",
                "--bin",
                "borea-live",
                "-p",
                "borea-bootstrap",
                "--bin",
                "borea",
                "-p",
                "borea-flash",
                "--bin",
                "borea-flash",
                "-p",
                "borea-installd",
                "--bin",
                "borea-installd",
                "-p",
                "borea-cli",
                "--bin",
                "borea-cli",
                "-p",
                "borea-init",
                "--bin",
                "borea-init",
                "--bin",
                "borea-session",
                "-p",
                "borea-build",
                "--bin",
                "borea-build",
            ],
        )?;
        exec(
            &container,
            &uid,
            &[
                "env",
                "RUSTFLAGS=-C target-feature=+crt-static",
                "/rust/bin/cargo",
                "build",
                "--locked",
                "--profile",
                "app",
                "--target",
                "x86_64-unknown-linux-gnu",
                "--target-dir",
                "/work/static",
                "-p",
                "borea-exec",
                "--bin",
                "borea-firstboot",
            ],
        )?;
    }
    let builder =
        if bins.is_some() { "/borea-bins/borea-build" } else { "/work/cargo/app/borea-build" };
    exec(&container, "0", &[builder, "compose-root"])?;
    let pending = work.join("out/borea-live-compose.iso.part");
    tool("docker", &["cp", &format!("{}:/out/borea-live.iso", container.0), arg(&pending)?])?;
    drop(container);
    let iso = work.join("out/borea-live-compose.iso");
    fs::rename(&pending, &iso).map_err(|e| e.to_string())?;
    write(
        iso.with_extension("iso.sha256"),
        format!("{}  borea-live-compose.iso\n", sha256(&iso)?),
    )?;
    println!("\nReady: {}", iso.display());
    Ok(())
}

/// Internal, container-only step. Never touches host devices or partitions.
pub fn root() -> Result {
    if !rustix::process::geteuid().is_root()
        || !Path::new("/.dockerenv").exists()
        || fs::read_to_string("/etc/borea-compose-runtime").unwrap_or_default()
            != "Borea composition runtime\n"
    {
        return Err("compose-root is only allowed inside Borea's composition container".into());
    }
    let root = Path::new("/tmp/borea-root");
    let repo = Path::new("/src");
    let bins = if Path::new("/borea-bins").is_dir() {
        Path::new("/borea-bins")
    } else {
        Path::new("/work/cargo/app")
    };
    println!(">>> compose 1 Assembling the Borea live session");
    tool("tar", &["-xf", "/src/target/live/compose-limine.tar.xz", "-C", "/tmp"])?;
    tool("make", &["-C", "/tmp/limine-binary", "limine"])?;
    copy(Path::new("/tmp/limine-binary/limine"), Path::new("/usr/bin/limine"))?;
    for name in ["BOOTX64.EFI", "limine-bios.sys", "limine-bios-cd.bin", "limine-uefi-cd.bin"] {
        copy(
            &Path::new("/tmp/limine-binary").join(name),
            &Path::new("/usr/share/limine").join(name),
        )?;
    }
    // These tools prepared Borea but are not part of the live environment.
    tool(
        "apt-get",
        &["purge", "-y", "--auto-remove", "build-essential", "gcc", "g++", "make", "pkg-config"],
    )?;
    stage_root(root)?;
    // Backend commands use a consistent /usr/bin layout, while upstream runtime
    // packages also place administration tools in /usr/sbin.
    for entry in fs::read_dir(root.join("usr/sbin")).map_err(|e| e.to_string())?.flatten() {
        let name = entry.file_name();
        let alias = root.join("usr/bin").join(&name);
        if fs::symlink_metadata(&alias).is_err() {
            symlink(Path::new("../sbin").join(name), alias).map_err(|e| e.to_string())?;
        }
    }
    for name in BINS {
        let to = if *name == "borea-init" {
            root.join("usr/lib/borea/borea-init")
        } else {
            root.join("usr/bin").join(name)
        };
        copy(&bins.join(name), &to)?;
        fs::set_permissions(to, fs::Permissions::from_mode(0o755)).map_err(|e| e.to_string())?;
    }
    let helper = if bins == Path::new("/borea-bins") {
        bins.join("borea-firstboot")
    } else {
        PathBuf::from("/work/static/x86_64-unknown-linux-gnu/app/borea-firstboot")
    };
    let data = fs::read(&helper).map_err(|e| e.to_string())?;
    let elf = goblin::elf::Elf::parse(&data).map_err(|e| e.to_string())?;
    if elf.interpreter.is_some() || !elf.libraries.is_empty() {
        return Err("borea-firstboot must be a statically linked binary".into());
    }
    copy(&helper, &root.join("usr/bin/borea-firstboot"))?;
    fs::set_permissions(root.join("usr/bin/borea-firstboot"), fs::Permissions::from_mode(0o755))
        .map_err(|e| e.to_string())?;
    overlay(&repo.join("live/buildroot/board/borea/rootfs-overlay"), root)?;
    for key in ["arch", "fedora", "opensuse", "omarchy"] {
        copy(
            &repo.join(format!("backend/dist/keys/{key}.gpg")),
            &root.join(format!("usr/share/keyrings/borea/{key}.gpg")),
        )?;
    }
    copy(
        &repo.join("backend/dist/borea-installd.service"),
        &root.join("usr/lib/systemd/system/borea-installd.service"),
    )?;
    copy(&repo.join("backend/dist/borea.tmpfiles"), &root.join("usr/lib/tmpfiles.d/borea.conf"))?;
    for path in ["usr/lib/systemd/system/borea-installd.service", "usr/lib/tmpfiles.d/borea.conf"] {
        fs::set_permissions(root.join(path), fs::Permissions::from_mode(0o644))
            .map_err(|e| e.to_string())?;
    }
    tool(
        "chroot",
        &[
            arg(root)?,
            "useradd",
            "--create-home",
            "--uid",
            "1000",
            "--shell",
            "/bin/bash",
            "--groups",
            "video,render,input",
            "borea",
        ],
    )?;
    tool("chroot", &[arg(root)?, "passwd", "--delete", "root"])?;
    root_link(root, "/lib/systemd/system/graphical.target", "etc/systemd/system/default.target")?;
    root_link(
        root,
        "../borea-live.service",
        "etc/systemd/system/graphical.target.wants/borea-live.service",
    )?;
    root_link(
        root,
        "/usr/lib/systemd/system/borea-installd.service",
        "etc/systemd/system/multi-user.target.wants/borea-installd.service",
    )?;
    root_link(
        root,
        "../sys-firmware-efi-efivars.mount",
        "etc/systemd/system/sysinit.target.wants/sys-firmware-efi-efivars.mount",
    )?;
    root_link(
        root,
        "/usr/lib/systemd/system/NetworkManager.service",
        "etc/systemd/system/multi-user.target.wants/NetworkManager.service",
    )?;
    root_link(root, "/run/systemd/resolve/stub-resolv.conf", "etc/resolv.conf")?;
    write(root.join("etc/hostname"), "borea-live\n")?;
    write(
        root.join("etc/hosts"),
        "127.0.0.1 localhost\n127.0.1.1 borea-live\n::1 localhost ip6-localhost\n",
    )?;
    write(root.join("etc/issue"), "Borea Live (composed)\\n \\l\n\n")?;
    write(root.join("etc/machine-id"), "")?;
    root_link(root, "/etc/machine-id", "var/lib/dbus/machine-id")?;
    write(root.join("etc/locale.conf"), "LANG=C.UTF-8\n")?;
    write(root.join("etc/borea-live"), "composed\n")?;
    write(
        root.join("usr/lib/os-release"),
        "NAME=\"Borea Live\"\nID=borea\nPRETTY_NAME=\"Borea Live (composed)\"\n",
    )?;
    let id = iso::build_id()?;
    write(root.join("etc/borea-live-build"), format!("{id}\n"))?;
    for lib in ["libEGL.so.1", "libwayland-egl.so.1"] {
        root_link(root, &format!("x86_64-linux-gnu/{lib}"), &format!("usr/lib/{lib}"))?;
    }
    let versions = fs::read_dir(root.join("usr/lib/modules"))
        .map_err(|e| e.to_string())?
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_dir())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    if versions.len() != 1 {
        return Err("the composition runtime must contain exactly one kernel".into());
    }
    let kernel = &versions[0];
    let mut modules = Vec::new();
    for name in MODULES {
        let status = command("modprobe")
            .args(["--dirname", arg(root)?, "--show-depends", "--set-version", kernel, name])
            .output()
            .map_err(|e| e.to_string())?;
        if status.status.success() {
            modules.push(*name);
        } else if ["erofs", "overlay", "isofs", "loop"].contains(name) {
            return Err(format!("the prebuilt kernel lacks {name}"));
        }
    }
    write(root.join("etc/borea-early-modules"), format!("{kernel}\n{}\n", modules.join("\n")))?;
    check_usb_network(root, kernel)?;
    fs::create_dir_all("/out").map_err(|e| e.to_string())?;
    copy(&root.join(format!("boot/vmlinuz-{kernel}")), Path::new("/out/bzImage"))?;
    write(root.join("etc/borea-compose-packages"), output("dpkg-query", &["-W"])?)?;
    for entry in fs::read_dir(root.join("boot")).map_err(|e| e.to_string())?.flatten() {
        if entry.path().is_file() {
            fs::remove_file(entry.path()).map_err(|e| e.to_string())?;
        }
    }
    // Preserve the sticky bit for users' temporary files on the writable overlay.
    fs::set_permissions(root.join("tmp"), fs::Permissions::from_mode(0o1777))
        .map_err(|e| e.to_string())?;
    hooks::check_root(root)?;
    println!(">>> compose 1 Packing the live filesystem");
    tool("mkfs.erofs", &["-zlz4", "/out/rootfs.erofs", arg(root)?])?;
    hooks::compose_image(
        Path::new("/out"),
        Path::new("/usr"),
        root,
        Path::new("/src/live/buildroot/board/borea"),
        &id,
    )
}

#[cfg(test)]
mod tests {
    #[test]
    fn root_finalization_refuses_the_build_host() {
        assert!(super::root().is_err());
    }

    #[test]
    fn overlay_permissions_do_not_depend_on_a_private_checkout() {
        use std::os::unix::fs::PermissionsExt;
        let root = std::env::temp_dir().join(format!("borea-overlay-mode-{}", std::process::id()));
        let source = root.join("source");
        let dest = root.join("dest");
        std::fs::create_dir_all(source.join("etc/systemd")).unwrap();
        std::fs::write(source.join("etc/systemd/example.conf"), "fixture").unwrap();
        for dir in [&source, &source.join("etc"), &source.join("etc/systemd")] {
            std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700)).unwrap();
        }
        std::fs::set_permissions(
            source.join("etc/systemd/example.conf"),
            std::fs::Permissions::from_mode(0o600),
        )
        .unwrap();
        super::overlay(&source, &dest).unwrap();
        for dir in [&dest, &dest.join("etc"), &dest.join("etc/systemd")] {
            assert_eq!(std::fs::metadata(dir).unwrap().permissions().mode() & 0o777, 0o755);
        }
        assert_eq!(
            std::fs::metadata(dest.join("etc/systemd/example.conf")).unwrap().permissions().mode()
                & 0o777,
            0o644
        );
        std::fs::remove_dir_all(root).unwrap();
    }
}
