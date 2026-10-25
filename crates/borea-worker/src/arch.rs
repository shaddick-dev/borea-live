//! Arch Linux, the way Arch installs itself: pacstrap from the signed bootstrap
//! archive, then configuration inside the new system with arch-chroot.

use std::fs::{self, File};
use std::io::BufReader;
use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use borea_engine::{InstallPlan, disk, fetch};
use borea_hardware::DiskUsage;

use crate::run::Log;

/// Where the new system is mounted, inside the worker's own namespace.
const TARGET: &str = "/run/borea/target";
/// The bootstrap archive and its unpacked tools live on the new disk while
/// installing (a live session's memory is too small), and are removed after.
const WORK: &str = ".borea";
const ESP_TYPE: &str = "C12A7328-F81F-11D2-BA4B-00A0C93EC93B";
/// Discoverable Partitions: Linux root (x86-64).
const ROOT_TYPE: &str = "4F68BCE3-E8CD-4DB1-96E7-FBCAF984B709";

const STEPS: usize = 11;

pub struct Install<'a> {
    plan: &'a InstallPlan,
    password: &'a str,
    bootstrap: Option<PathBuf>,
    log: Log,
    target: PathBuf,
    /// The unpacked bootstrap: Arch's own tools, used through chroot.
    tools: PathBuf,
}

/// The partitions created, by device path.
struct Partitions {
    esp: String,
    root: String,
}

impl<'a> Install<'a> {
    pub fn new(
        plan: &'a InstallPlan,
        password: &'a str,
        bootstrap: Option<PathBuf>,
        log: Log,
    ) -> Self {
        let target = PathBuf::from(TARGET);
        let tools = target.join(WORK).join("root.x86_64");
        Install { plan, password, bootstrap, log, target, tools }
    }

    pub fn run(mut self) -> Result<(), String> {
        let started = Instant::now();
        let result = self.steps();
        // Whatever happened, leave nothing mounted (the namespace would anyway).
        self.unmount_all();
        match &result {
            Ok(()) => println!(
                "\nArch Linux is installed on {} in {}:{:02}. Remove the installer and restart.",
                self.plan.disk.name,
                started.elapsed().as_secs() / 60,
                started.elapsed().as_secs() % 60
            ),
            Err(_) => println!("\nThe installation stopped. The disk may be partly written."),
        }
        result
    }

    fn steps(&mut self) -> Result<(), String> {
        let disk = self.check_target()?;
        let parts = self.partition(&disk)?;
        self.format(&parts)?;
        self.mount(&parts)?;
        self.unpack_bootstrap()?;
        self.prepare_tools()?;
        self.pacstrap()?;
        self.configure(&parts)?;
        self.boot(&disk, &parts)?;
        self.validate(&parts)?;
        self.finish()
    }

    // ── 1. The disk is still the one approved, and still free ────────────────

    fn check_target(&mut self) -> Result<String, String> {
        let d = &self.plan.disk;
        self.log.step(STEPS, &format!("Checking {} is the disk you approved", d.stable_path));
        let name = disk::resolve(Path::new("/"), &d.stable_path)
            .ok_or_else(|| format!("{} is gone: plug it back in, or plan again", d.stable_path))?;
        let hw = borea_hardware::scan();
        let now =
            hw.disks.iter().find(|x| x.name == name).ok_or_else(|| format!("{name} vanished"))?;
        if now.size_bytes != d.size_bytes {
            return Err(format!(
                "{name} is now {} bytes, not {}: plan again",
                now.size_bytes, d.size_bytes
            ));
        }
        if now.usage != DiskUsage::Free {
            return Err(format!("{name} is in use now ({}): nothing was changed", now.usage));
        }
        Ok(name)
    }

    // ── 2–4. Partition, format, mount ────────────────────────────────────────

    fn partition(&mut self, disk: &str) -> Result<Partitions, String> {
        let dev = format!("/dev/{disk}");
        self.log.step(STEPS, &format!("Erasing {dev} and creating its partitions"));
        let script = format!(
            "label: gpt\nsize={}MiB, type={ESP_TYPE}, name=\"EFI system partition\"\ntype={ROOT_TYPE}, name=\"Arch Linux\"\n",
            self.plan.layout.esp_mib
        );
        self.log.run(
            "/usr/bin/sfdisk",
            &["--wipe", "always", "--wipe-partitions", "always", "--quiet", &dev],
            Some(script.as_bytes()),
        )?;
        self.log.run("/usr/bin/udevadm", &["settle", "--timeout=30"], None)?;
        let found = |n| disk::partition(Path::new("/"), disk, n).map(|p| format!("/dev/{p}"));
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            if let (Some(esp), Some(root)) = (found(1), found(2))
                && Path::new(&root).exists()
            {
                return Ok(Partitions { esp, root });
            }
            if Instant::now() > deadline {
                return Err(format!("the new partitions on {dev} never appeared"));
            }
            std::thread::sleep(Duration::from_millis(200));
        }
    }

    fn format(&mut self, p: &Partitions) -> Result<(), String> {
        self.log.step(STEPS, "Creating the file systems");
        self.log.run("/usr/bin/mkfs.fat", &["-F", "32", "-n", "ESP", &p.esp], None)?;
        self.log.run("/usr/bin/mkfs.ext4", &["-F", "-q", "-L", "arch", &p.root], None)?;
        Ok(())
    }

    fn mount(&mut self, p: &Partitions) -> Result<(), String> {
        self.log.step(STEPS, "Mounting the new system");
        let target = self.target.to_string_lossy().into_owned();
        mkdir(&self.target)?;
        self.log.run("/usr/bin/mount", &["-t", "ext4", &p.root, &target], None)?;
        mkdir(&self.target.join("boot"))?;
        let boot = format!("{target}/boot");
        self.log.run(
            "/usr/bin/mount",
            &["-t", "vfat", "-o", "fmask=0077,dmask=0077", &p.esp, &boot],
            None,
        )?;
        Ok(())
    }

    // ── 5–6. Arch's own tools ────────────────────────────────────────────────

    fn unpack_bootstrap(&mut self) -> Result<(), String> {
        let a = &self.plan.bootstrap;
        self.log.step(STEPS, &format!("Getting Arch's bootstrap tools ({})", a.release));
        let work = self.target.join(WORK);
        mkdir(&work)?;
        let archive = work.join("bootstrap.tar.zst");
        // Copy (or download) while hashing, so the bytes checked are the bytes used.
        let (sha256, _) = match &self.bootstrap {
            Some(from) => {
                let mut src = File::open(from).map_err(|e| format!("{}: {e}", from.display()))?;
                let mut dst = File::create(&archive).map_err(|e| e.to_string())?;
                fetch::copy_hashing(&mut src, &mut dst, &mut |_| {}).map_err(|e| e.to_string())?
            }
            None => fetch::download(&a.url, &archive, a.size, |_| {})?,
        };
        if sha256 != a.sha256 {
            return Err(format!(
                "the bootstrap archive's SHA-256 is {sha256}, not the approved {}",
                a.sha256
            ));
        }
        self.log.note("matches the approved SHA-256; unpacking");
        let file = File::open(&archive).map_err(|e| e.to_string())?;
        let zstd = ruzstd::decoding::StreamingDecoder::new(BufReader::new(file))
            .map_err(|e| format!("bootstrap archive: {e}"))?;
        let mut tar = tar::Archive::new(zstd);
        tar.set_preserve_permissions(true);
        tar.set_preserve_ownerships(true);
        tar.set_unpack_xattrs(false);
        tar.unpack(&work).map_err(|e| format!("unpacking the bootstrap archive: {e}"))?;
        fs::remove_file(&archive).map_err(|e| e.to_string())?;
        if !self.tools.join("usr/bin/pacstrap").is_file() {
            return Err("the bootstrap archive has no pacstrap".into());
        }
        Ok(())
    }

    fn prepare_tools(&mut self) -> Result<(), String> {
        self.log.step(STEPS, "Preparing Arch's tools and package keys");
        let tools = self.tools.clone();
        write(
            &tools.join("etc/pacman.d/mirrorlist"),
            &format!("Server = {}/$repo/os/$arch\n", self.plan.mirror),
        )?;
        // The live session's resolver, as content: it may be a symlink into /run.
        let resolv =
            fs::read_to_string("/etc/resolv.conf").map_err(|e| format!("/etc/resolv.conf: {e}"))?;
        let _ = fs::remove_file(tools.join("etc/resolv.conf"));
        write(&tools.join("etc/resolv.conf"), &resolv)?;
        let conf = tools.join("etc/pacman.conf");
        let pacman = fs::read_to_string(&conf).map_err(|e| e.to_string())?;
        write(&conf, &pacman.replace("#ParallelDownloads", "ParallelDownloads"))?;

        // The new system first, so its view holds only its own two file systems.
        let t = |p: &str| tools.join(p).to_string_lossy().into_owned();
        let target = self.target.to_string_lossy().into_owned();
        self.log.run("/usr/bin/mount", &["--rbind", &target, &t("mnt")], None)?;
        self.log.run("/usr/bin/mount", &["-t", "proc", "proc", &t("proc")], None)?;
        self.log.run("/usr/bin/mount", &["--rbind", "/sys", &t("sys")], None)?;
        self.log.run("/usr/bin/mount", &["--rbind", "/dev", &t("dev")], None)?;
        self.log.run("/usr/bin/mount", &["-t", "tmpfs", "tmpfs", &t("tmp")], None)?;
        self.log.run("/usr/bin/mount", &["-t", "tmpfs", "tmpfs", &t("run")], None)?;

        self.tool(&["pacman-key", "--init"], None)?;
        self.tool(&["pacman-key", "--populate", "archlinux"], None)?;
        // pacman-key leaves gpg-agent running, which would keep the tools busy.
        self.tool(&["gpgconf", "--homedir", "/etc/pacman.d/gnupg", "--kill", "all"], None)?;
        Ok(())
    }

    /// Run one of Arch's own tools from the bootstrap.
    fn tool(&self, command: &[&str], input: Option<&[u8]>) -> Result<String, String> {
        let root = self.tools.to_string_lossy().into_owned();
        let mut args = vec![root.as_str()];
        args.extend_from_slice(command);
        self.log.run("/usr/bin/chroot", &args, input)
    }

    /// Run a command inside the new system.
    fn inside(&self, command: &[&str], input: Option<&[u8]>) -> Result<String, String> {
        let mut args = vec!["arch-chroot", "/mnt"];
        args.extend_from_slice(command);
        self.tool(&args, input)
    }

    // ── 7. Packages ──────────────────────────────────────────────────────────

    fn pacstrap(&mut self) -> Result<(), String> {
        self.log.step(
            STEPS,
            &format!("Installing {} packages and their dependencies", self.plan.packages.len()),
        );
        self.log.note(&self.plan.packages.join(" "));
        let mut args = vec!["pacstrap", "-K", "/mnt"];
        args.extend(self.plan.packages.iter().map(String::as_str));
        self.tool(&args, None)?;
        // The new system's keyring, set up by -K, leaves its own gpg-agent too.
        self.inside(&["gpgconf", "--homedir", "/etc/pacman.d/gnupg", "--kill", "all"], None)?;
        Ok(())
    }

    // ── 8. The system's settings ─────────────────────────────────────────────

    fn configure(&mut self, p: &Partitions) -> Result<(), String> {
        let s = &self.plan.system;
        let account =
            borea_engine::account::setup(borea_engine::online::Target::Arch, &s.username)?;
        self.log.step(STEPS, &format!("Setting up {} for {}", s.hostname, s.username));
        let t = &self.target;
        let root_uuid = self.uuid(&p.root)?;
        let esp_uuid = self.uuid(&p.esp)?;
        write(
            &t.join("etc/fstab"),
            &format!(
                "# Written by Borea: file systems by UUID.\n\
                 UUID={root_uuid}\t/\text4\trw,relatime\t0 1\n\
                 UUID={esp_uuid}\t/boot\tvfat\trw,relatime,fmask=0077,dmask=0077,codepage=437,iocharset=ascii,shortname=mixed,utf8,errors=remount-ro\t0 2\n"
            ),
        )?;
        let _ = fs::remove_file(t.join("etc/localtime"));
        symlink(format!("/usr/share/zoneinfo/{}", s.timezone), t.join("etc/localtime"))
            .map_err(|e| format!("/etc/localtime: {e}"))?;
        let charset = s.locale.split_once('.').map_or("UTF-8", |(_, c)| c);
        append(&t.join("etc/locale.gen"), &format!("\n{} {charset}\n", s.locale))?;
        write(&t.join("etc/locale.conf"), &format!("LANG={}\n", s.locale))?;
        write(&t.join("etc/vconsole.conf"), &format!("KEYMAP={}\n", s.keymap))?;
        write(&t.join("etc/hostname"), &format!("{}\n", s.hostname))?;
        write(
            &t.join("etc/hosts"),
            &format!("127.0.0.1\tlocalhost\n::1\t\tlocalhost\n127.0.1.1\t{}\n", s.hostname),
        )?;
        let sudoers = t.join("etc/sudoers.d/10-borea-wheel");
        write(&sudoers, &account.sudoers)?;
        fs::set_permissions(&sudoers, fs::Permissions::from_mode(0o440))
            .map_err(|e| e.to_string())?;

        self.inside(&["locale-gen"], None)?;
        self.inside(&account.create_user.iter().map(String::as_str).collect::<Vec<_>>(), None)?;
        let secret = format!("{}:{}\n", s.username, self.password);
        self.inside(&["chpasswd"], Some(secret.as_bytes()))?;
        self.inside(&account.lock_root.iter().map(String::as_str).collect::<Vec<_>>(), None)?;
        for service in &self.plan.services {
            self.inside(&["systemctl", "enable", service], None)?;
        }
        // Again, now the console keymap and locale exist.
        self.inside(&["mkinitcpio", "-P"], None)?;
        Ok(())
    }

    fn uuid(&self, device: &str) -> Result<String, String> {
        let out = self.log.run("/usr/bin/blkid", &["-s", "UUID", "-o", "value", device], None)?;
        let uuid = out.trim().to_owned();
        if uuid.is_empty() { Err(format!("{device} has no UUID")) } else { Ok(uuid) }
    }

    // ── 9. Limine ────────────────────────────────────────────────────────────

    fn boot(&mut self, disk: &str, p: &Partitions) -> Result<(), String> {
        self.log.step(STEPS, "Installing Limine on the EFI partition");
        let t = &self.target;
        let efi = t.join("usr/share/limine/BOOTX64.EFI");
        for dest in ["boot/EFI/limine/BOOTX64.EFI", "boot/EFI/BOOT/BOOTX64.EFI"] {
            copy(&efi, &t.join(dest))?;
        }
        let root_uuid = self.uuid(&p.root)?;
        write(
            &t.join("boot/limine.conf"),
            &limine_conf(self.plan, &root_uuid, |f| t.join("boot").join(f).is_file()),
        )?;
        write(&t.join("etc/pacman.d/hooks/90-borea-limine.hook"), LIMINE_HOOK)?;

        // A firmware boot entry; the fallback path above boots even without one.
        let entry = self.inside(
            &[
                "efibootmgr",
                "--create",
                "--disk",
                &format!("/dev/{disk}"),
                "--part",
                "1",
                "--label",
                "Arch Linux",
                "--loader",
                "\\EFI\\limine\\BOOTX64.EFI",
                "--unicode",
            ],
            None,
        );
        match entry {
            Ok(_) => self.log.note("firmware boot entry \"Arch Linux\" added"),
            Err(_) => self.log.note(
                "the firmware refused a new boot entry; it will start from EFI/BOOT/BOOTX64.EFI",
            ),
        }
        Ok(())
    }

    // ── 10. Check before saying it worked ────────────────────────────────────

    fn validate(&mut self, p: &Partitions) -> Result<(), String> {
        self.log.step(STEPS, "Checking the new system");
        let t = &self.target;
        let conf = fs::read_to_string(t.join("boot/limine.conf")).map_err(|e| e.to_string())?;
        for line in conf.lines() {
            if let Some(file) = line.trim().split_once("boot():/").map(|(_, f)| f) {
                if !t.join("boot").join(file).is_file() {
                    return Err(format!("limine.conf points to /boot/{file}, which is missing"));
                }
            }
        }
        let fstab = fs::read_to_string(t.join("etc/fstab")).map_err(|e| e.to_string())?;
        for device in [&p.root, &p.esp] {
            let uuid = self.uuid(device)?;
            if !fstab.contains(&uuid) {
                return Err(format!("/etc/fstab lacks {device} (UUID {uuid})"));
            }
        }
        let passwd = fs::read_to_string(t.join("etc/passwd")).map_err(|e| e.to_string())?;
        if !passwd.lines().any(|l| l.starts_with(&format!("{}:", self.plan.system.username))) {
            return Err("the user was not created".into());
        }
        for service in &self.plan.services {
            let wanted = t.join("etc/systemd/system/multi-user.target.wants").join(service);
            if fs::symlink_metadata(&wanted).is_err() {
                return Err(format!("{service} is not enabled"));
            }
        }
        for file in ["boot/EFI/BOOT/BOOTX64.EFI", "boot/EFI/limine/BOOTX64.EFI"] {
            if !t.join(file).is_file() {
                return Err(format!("/{file} is missing"));
            }
        }
        self.log.note("boot files, fstab, user, and services are in place");
        Ok(())
    }

    // ── 11. Tidy up ──────────────────────────────────────────────────────────

    fn finish(&mut self) -> Result<(), String> {
        self.log.step(STEPS, "Removing the bootstrap tools and unmounting");
        self.unmount_tools();
        let work = self.target.join(WORK);
        // The tools' /mnt was the new system itself: deleting through a mount
        // that failed to come off would delete the system. Refuse instead.
        let still = mounts_under(&work, &read_mounts()?);
        if !still.is_empty() {
            return Err(format!(
                "{} is still mounted, so the bootstrap tools were left in /{WORK}",
                still.join(", ")
            ));
        }
        remove_tree(&work).map_err(|e| format!("removing /{WORK}: {e}"))?;
        self.log.run("/usr/bin/sync", &[], None)?;
        Ok(())
    }

    fn unmount_tools(&self) {
        for dir in ["run", "tmp", "dev", "sys", "proc", "mnt"] {
            self.unmount(&self.tools.join(dir));
        }
    }

    /// Unmount everything at and under `path`, giving a process that is still
    /// exiting a few seconds to let go.
    fn unmount(&self, path: &Path) {
        let path = path.to_string_lossy();
        for attempt in 0..10 {
            let mounted = fs::read_to_string("/proc/self/mounts")
                .is_ok_and(|m| m.lines().any(|l| l.split(' ').nth(1) == Some(&*path)));
            if !mounted || self.log.run("/usr/bin/umount", &["--recursive", &path], None).is_ok() {
                return;
            }
            std::thread::sleep(Duration::from_millis(200 * (attempt + 1)));
        }
    }

    fn unmount_all(&self) {
        self.unmount_tools();
        self.unmount(&self.target);
    }
}

/// Keeps the EFI copies of Limine current when the package updates. Kernels need
/// no hook: they keep their names on the EFI partition, which limine.conf uses.
const LIMINE_HOOK: &str = "\
# Written by Borea: copy Limine to the EFI partition whenever its package changes.
[Trigger]
Operation = Install
Operation = Upgrade
Type = Package
Target = limine

[Action]
Description = Updating Limine on the EFI partition...
When = PostTransaction
Exec = /usr/bin/sh -c 'install -Dm644 /usr/share/limine/BOOTX64.EFI /boot/EFI/limine/BOOTX64.EFI && install -Dm644 /usr/share/limine/BOOTX64.EFI /boot/EFI/BOOT/BOOTX64.EFI'
";

/// Limine's menu: the kernel, microcode first, and a fallback entry when
/// mkinitcpio made a fallback image. `exists` says whether /boot has a file.
fn limine_conf(plan: &InstallPlan, root_uuid: &str, exists: impl Fn(&str) -> bool) -> String {
    let mut cmdline = format!("root=UUID={root_uuid} rw");
    for arg in &plan.boot.kernel_args {
        cmdline.push(' ');
        cmdline.push_str(arg);
    }
    let entry = |title: &str, initramfs: &str| {
        let mut text = format!(
            "/{title}\n    protocol: linux\n    path: boot():/vmlinuz-linux\n    cmdline: {cmdline}\n"
        );
        if let Some(microcode) = &plan.boot.microcode {
            text.push_str(&format!("    module_path: boot():/{microcode}\n"));
        }
        text.push_str(&format!("    module_path: boot():/{initramfs}\n"));
        text
    };
    let mut conf = String::from("# Written by Borea.\ntimeout: 3\n\n");
    conf.push_str(&entry("Arch Linux", "initramfs-linux.img"));
    if exists("initramfs-linux-fallback.img") {
        conf.push('\n');
        conf.push_str(&entry("Arch Linux (fallback initramfs)", "initramfs-linux-fallback.img"));
    }
    conf
}

/// This namespace's mount table. Unreadable is an error, never "nothing mounted".
fn read_mounts() -> Result<String, String> {
    fs::read_to_string("/proc/self/mounts").map_err(|e| format!("/proc/self/mounts: {e}"))
}

/// Mount points at or under `dir`, from a mount table (`/proc/self/mounts`).
fn mounts_under(dir: &Path, table: &str) -> Vec<String> {
    let dir = dir.to_string_lossy();
    table
        .lines()
        .filter_map(|l| l.split(' ').nth(1))
        .filter(|m| *m == dir || m.starts_with(&format!("{dir}/")))
        .map(String::from)
        .collect()
}

/// Delete `root` and everything in it, but never anything on another file
/// system: a mount point inside stops the whole removal.
fn remove_tree(root: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::MetadataExt;
    let device = fs::symlink_metadata(root)?.dev();
    fn walk(path: &Path, device: u64) -> std::io::Result<()> {
        let meta = fs::symlink_metadata(path)?;
        if meta.dev() != device {
            return Err(std::io::Error::other(format!(
                "{} is another file system",
                path.display()
            )));
        }
        if meta.is_dir() {
            for entry in fs::read_dir(path)? {
                walk(&entry?.path(), device)?;
            }
            fs::remove_dir(path)
        } else {
            fs::remove_file(path)
        }
    }
    walk(root, device)
}

fn mkdir(path: &Path) -> Result<(), String> {
    fs::create_dir_all(path).map_err(|e| format!("{}: {e}", path.display()))
}

fn write(path: &Path, text: &str) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        mkdir(dir)?;
    }
    fs::write(path, text).map_err(|e| format!("{}: {e}", path.display()))
}

fn append(path: &Path, text: &str) -> Result<(), String> {
    use std::io::Write;
    let mut file = fs::OpenOptions::new()
        .append(true)
        .open(path)
        .map_err(|e| format!("{}: {e}", path.display()))?;
    file.write_all(text.as_bytes()).map_err(|e| format!("{}: {e}", path.display()))
}

fn copy(from: &Path, to: &Path) -> Result<(), String> {
    if let Some(dir) = to.parent() {
        mkdir(dir)?;
    }
    fs::copy(from, to)
        .map(|_| ())
        .map_err(|e| format!("{} → {}: {e}", from.display(), to.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use borea_engine::plan::{Artifact, Boot, Layout, System, TargetDisk};

    fn plan(microcode: Option<&str>, args: &[&str]) -> InstallPlan {
        InstallPlan {
            schema: 1,
            distro: "arch".into(),
            edition: "minimal".into(),
            disk: TargetDisk {
                name: "vda".into(),
                stable_path: "/dev/disk/by-path/x".into(),
                size_bytes: 1,
                model: None,
            },
            layout: Layout { esp_mib: 2048, root_fs: "ext4".into() },
            bootstrap: Artifact {
                release: "r".into(),
                url: "u".into(),
                sha256: "s".into(),
                size: 1,
                signed_by: "k".into(),
            },
            mirror: "m".into(),
            packages: vec![],
            system: System {
                hostname: "h".into(),
                timezone: "UTC".into(),
                locale: "C.UTF-8".into(),
                keymap: "us".into(),
                username: "u".into(),
            },
            boot: Boot {
                loader: "limine".into(),
                microcode: microcode.map(String::from),
                kernel_args: args.iter().map(|a| a.to_string()).collect(),
            },
            services: vec![],
        }
    }

    /// Run 1 of the VM test: the tools' /mnt (the new system) failed to unmount,
    /// and removing the tools deleted the system through it. The check sees it.
    #[test]
    fn a_mount_left_under_the_tools_blocks_their_removal() {
        let table = "\
/dev/vda2 /run/borea/target ext4 rw 0 0
/dev/vda1 /run/borea/target/boot vfat rw 0 0
/dev/vda2 /run/borea/target/.borea/root.x86_64/mnt ext4 rw 0 0
/dev/vda2 /run/borea/target/.borealis ext4 rw 0 0
";
        let work = Path::new("/run/borea/target/.borea");
        assert_eq!(mounts_under(work, table), ["/run/borea/target/.borea/root.x86_64/mnt"]);
        assert!(mounts_under(work, "/dev/vda2 /run/borea/target ext4 rw 0 0\n").is_empty());
    }

    #[test]
    fn removal_stays_on_one_file_system() {
        let dir = std::env::temp_dir().join(format!("borea-tree-{}", std::process::id()));
        fs::create_dir_all(dir.join("a/b")).unwrap();
        fs::write(dir.join("a/b/file"), "x").unwrap();
        std::os::unix::fs::symlink("/etc", dir.join("a/etc")).unwrap();
        remove_tree(&dir).unwrap();
        assert!(!dir.exists());
        assert!(Path::new("/etc").is_dir(), "symlinks are removed, never followed");
    }

    #[test]
    fn limine_loads_microcode_before_the_initramfs() {
        let conf =
            limine_conf(&plan(Some("intel-ucode.img"), &["console=ttyS0"]), "1234", |_| false);
        assert!(conf.contains("cmdline: root=UUID=1234 rw console=ttyS0\n"));
        let micro = conf.find("intel-ucode.img").unwrap();
        let initramfs = conf.find("initramfs-linux.img").unwrap();
        assert!(micro < initramfs);
        assert!(!conf.contains("fallback"));
    }

    #[test]
    fn a_fallback_entry_only_when_its_image_exists() {
        let conf = limine_conf(&plan(None, &[]), "1234", |f| f == "initramfs-linux-fallback.img");
        assert!(conf.contains("/Arch Linux (fallback initramfs)"));
        assert!(!conf.contains("ucode"));
    }
}
