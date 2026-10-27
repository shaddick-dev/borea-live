#![forbid(unsafe_code)]
//! A reviewable, deterministic sequence. This crate never invokes a process or writes a file.
mod render;
use borea_backend::{Artifact, Bootstrap};
use borea_core::{DiskPlan, Distribution as Distro, Hint, Plan, Review, hint};
use borea_disk::{Change, DiskFacts, Layout};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
pub const TARGET: &str = "/run/borea/target";
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Root {
    Live,
    Target,
    Tools,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SecretRef {
    Luks,
    Login,
    RootPassword,
    Mok,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommandSpec {
    pub program: String,
    pub args: Vec<String>,
    pub root: Root,
    pub stdin: Option<SecretRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stdin_text: Option<String>,
    pub capture: Option<String>,
    pub timeout: u64,
    pub retry: bool,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Action {
    Run(CommandSpec),
    WriteFile { path: String, contents: String, mode: u32 },
    AcquireToolRoot { artifact: Artifact },
    Mount { source: String, path: String, fs: Option<String>, options: String },
    Unmount { path: String },
    PrepareExternal,
    ImportRepositoryKeys,
    PreflightNative,
    DeferInitramfs,
    RestoreInitramfs,
    EnrollMok,
    CopyNativeRepositories,
    InstallEfiFallback { shim: String, grub: String, mok: String, config: String },
    PreserveEfiFallback,
    RestoreEfiFallback,
    SignEfiFallback,
    InstallHelpers,
    ConfigureFstab,
    CopyNetwork,
    RestoreNetwork,
    RestoreTransportTrust,
    BuildAur { packages: Vec<String> },
    InstallAppImages { releases: Vec<String> },
    Note { message: String },
}
impl Action {
    pub fn preparation_only(&self) -> bool {
        matches!(self, Self::PrepareExternal | Self::AcquireToolRoot { .. } | Self::PreflightNative)
            || matches!(self, Self::Run(command) if command.root == Root::Live && command.program == "/usr/sbin/debootstrap" && command.args.first().is_some_and(|arg| arg == "--print-debs") && command.stdin.is_none() && command.stdin_text.is_none())
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Stage {
    pub title: String,
    pub weight: u32,
    pub actions: Vec<Action>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Job {
    pub secure_boot: bool,
    pub firmware_secure_boot: bool,
    pub setup_mode: Option<bool>,
    pub external: borea_backend::external::ExternalSources,
    pub version: u32,
    pub plan: Plan,
    pub review: Review,
    pub layout: Layout,
    pub bootstrap: Bootstrap,
    pub stages: Vec<Stage>,
}
impl Job {
    pub fn digest(&self) -> Result<String, Hint> {
        let bytes = serde_json::to_vec(self).map_err(|e| hint("job", e.to_string()))?;
        Ok(format!("{:x}", Sha256::digest(bytes)))
    }
    pub fn render_script(&self) -> String {
        let mut out = String::from(
            "#!/usr/bin/env bash\n# REVIEW ONLY: secrets and runtime identifiers are placeholders.\n# Execution is exclusively through the approved privileged service.\nexit 64\n\nset -euo pipefail\n",
        );
        for stage in &self.stages {
            out.push_str(&format!("\n# {}\n", stage.title));
            for action in &stage.actions {
                match action {
                    Action::Run(c) => {
                        if c.stdin.is_some() {
                            out.push_str("# stdin: <redacted secret>\n");
                        }
                        if let Some(input) = &c.stdin_text {
                            out.push_str(&format!("# public stdin: {}\n", input.trim_end()));
                        }
                        out.push_str(&format!(
                            "{}{} {}\n",
                            match c.root {
                                Root::Live => String::new(),
                                Root::Target => "chroot /run/borea/target ".into(),
                                Root::Tools => format!(
                                    "chroot {} ",
                                    quote(&format!("/run/borea/tools/{}", self.plan.distro.key()))
                                ),
                            },
                            quote(&c.program),
                            c.args.iter().map(|s| quote(s)).collect::<Vec<_>>().join(" ")
                        ));
                    }
                    Action::WriteFile { path, contents, mode } => out.push_str(&format!(
                        "# write target:{} mode {:o}\n# {}\n",
                        path,
                        mode,
                        contents.replace('\n', "\n# ")
                    )),
                    _ => render::helper(self, action, &mut out),
                }
            }
        }
        out.push_str("\n# Cleanup: stop child processes only in the job-owned private namespace,\n# unmount owned trees deepest first and close only this job's encrypted mapping.\n# Restore saved EFI/DNS configuration and preserve the redacted install log.\n");
        out.lines().map(str::trim_end).collect::<Vec<_>>().join("\n") + "\n"
    }
}
pub fn quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}
fn run(root: Root, program: &str, args: impl IntoIterator<Item = impl Into<String>>) -> Action {
    Action::Run(CommandSpec {
        program: program.into(),
        args: args.into_iter().map(Into::into).collect(),
        root,
        stdin: None,
        stdin_text: None,
        capture: None,
        timeout: if program == "/usr/sbin/debootstrap" { 14400 } else { 3600 },
        retry: true,
    })
}
fn target(program: &str, args: impl IntoIterator<Item = impl Into<String>>) -> Action {
    run(Root::Target, program, args)
}
fn write(path: &str, contents: String, mode: u32) -> Action {
    Action::WriteFile { path: path.into(), contents, mode }
}
fn secret(program: &str, args: &[&str], input: SecretRef) -> Action {
    let mut a = run(Root::Live, program, args.iter().copied());
    if let Action::Run(c) = &mut a {
        c.stdin = Some(input);
        c.retry = false;
    }
    a
}
fn capture(root: Root, program: &str, args: &[&str], name: &str) -> Action {
    let mut a = run(root, program, args.iter().copied());
    if let Action::Run(c) = &mut a {
        c.capture = Some(name.into());
    }
    a
}
fn install(d: Distro, pkgs: Vec<String>) -> Action {
    let (p, a) = borea_backend::install(d, &pkgs);
    target(&p, a)
}
pub fn compile(
    plan: &Plan,
    hw: &borea_catalog::Hardware,
    facts: &DiskFacts,
    bootstrap: &Bootstrap,
) -> Result<Job, Hint> {
    bootstrap.validate()?;
    if bootstrap.distro != plan.distro || bootstrap.release != plan.release {
        return Err(hint(
            "bootstrap",
            "The tool root does not match the selected distro and release.",
        ));
    }
    let mut review = borea_core::review(plan, hw)?;
    let secure_boot = plan.target_secure_boot(hw);
    if secure_boot && !hw.secure_boot {
        review.warnings.push("Signed boot is being prepared; enable Secure Boot in firmware after installation if it remains off.".into());
    }

    let layout = borea_disk::layout(&plan.disk, facts, plan.distro, plan.desktop.is_some())?;
    let base = borea_catalog::repos_for(plan.distro)
        .find(|r| r.base)
        .ok_or_else(|| hint("sources", "Missing base repository."))?
        .id;
    for p in borea_backend::boot_packages(plan.distro) {
        if !review.packages.iter().any(|v| v.name == *p) {
            review.packages.push(borea_core::Package { source: base.into(), name: (*p).into() });
        }
    }
    if secure_boot && plan.distro == Distro::Arch {
        for name in ["sbctl", "sbsigntools", "systemd-ukify"] {
            if !review.packages.iter().any(|p| p.name == name) {
                review
                    .packages
                    .push(borea_core::Package { source: base.into(), name: name.into() });
            }
        }
    }
    if review.packages.iter().any(|p| p.source == "aur") {
        for name in ["base-devel", "git"] {
            if !review.packages.iter().any(|p| p.name == name) {
                review
                    .packages
                    .push(borea_core::Package { source: base.into(), name: name.into() });
            }
        }
    }
    for name in [
        review.packages.iter().any(|p| p.source == "flathub").then_some("flatpak"),
        (review.mok && plan.distro != Distro::Arch).then_some("mokutil"),
    ]
    .into_iter()
    .flatten()
    {
        if !review.packages.iter().any(|p| p.name == name) {
            review.packages.push(borea_core::Package { source: base.into(), name: name.into() });
        }
    }
    let locale_package = match plan.distro {
        Distro::Fedora => Some(format!(
            "glibc-langpack-{}",
            plan.locale.lang.split(['_', '.', '@']).next().unwrap_or("en")
        )),
        Distro::OpenSuse => Some("glibc-locale".into()),
        _ => None,
    };
    if let Some(name) = locale_package {
        if !review.packages.iter().any(|p| p.name == name) {
            review.packages.push(borea_core::Package { source: base.into(), name });
        }
    }
    review.packages.sort_by(|a, b| (&a.source, &a.name).cmp(&(&b.source, &b.name)));
    let mut stages = Vec::new();
    let mut prepare = Vec::new();
    if let Some(artifact) = &bootstrap.artifact {
        prepare.push(Action::AcquireToolRoot { artifact: artifact.clone() });
    }
    prepare.push(Action::PrepareExternal);
    if plan.distro == Distro::Debian {
        let mut components = vec!["main"];
        for component in ["contrib", "non-free", "non-free-firmware"] {
            let source = format!("debian.{component}");
            if review.packages.iter().any(|p| p.source == source)
                || plan.sources.get(&source) == Some(&true)
            {
                components.push(component);
            }
        }
        let packages = review
            .packages
            .iter()
            .filter(|p| p.source.starts_with("debian."))
            .map(|p| p.name.split(':').next().unwrap_or(&p.name))
            .collect::<Vec<_>>();
        prepare.push(run(
            Root::Live,
            "/usr/sbin/debootstrap",
            [
                "--print-debs".into(),
                "--variant=minbase".into(),
                "--keyring=/usr/share/keyrings/debian-archive-keyring.gpg".into(),
                format!("--components={}", components.join(",")),
                format!("--include={}", packages.join(",")),
                "trixie".into(),
                "/run/borea/debian-preflight".into(),
                "https://deb.debian.org/debian".into(),
            ],
        ));
    }
    if plan.distro != Distro::Debian {
        prepare.push(Action::PreflightNative);
    }
    stages.push(Stage {
        title: "Verifying the native installation tools.".into(),
        weight: 0,
        actions: prepare,
    });
    let disk = layout.disk.path.to_string_lossy().to_string();
    let mut storage = Vec::new();
    for change in &layout.changes {
        match change {
            Change::Wipe => {
                let mut a = run(Root::Live, "/usr/sbin/sgdisk", ["--zap-all", &disk]);
                if let Action::Run(c) = &mut a {
                    c.retry = false;
                }
                storage.push(a);
            }
            Change::ResizeNtfs { path, new_size, number, .. } => {
                storage.push(run(
                    Root::Live,
                    "/usr/sbin/ntfsresize",
                    [
                        "--no-progress-bar".into(),
                        "--force".into(),
                        "--size".into(),
                        new_size.to_string(),
                        path.display().to_string(),
                    ],
                ));
                let mut resize = run(
                    Root::Live,
                    "/usr/sbin/sfdisk",
                    [
                        "--no-reread".into(),
                        "--no-tell-kernel".into(),
                        "--wipe=never".into(),
                        "--wipe-partitions=never".into(),
                        "-N".into(),
                        number.to_string(),
                        disk.clone(),
                    ],
                );
                if let Action::Run(command) = &mut resize {
                    command.stdin_text =
                        Some(format!("size={}\n", new_size / layout.disk.logical_sector));
                    command.retry = false;
                }
                storage.push(resize);
            }
            Change::Create { number, start_sector, end_sector, type_code, label, .. } => storage
                .push(run(
                    Root::Live,
                    "/usr/sbin/sgdisk",
                    [
                        format!("--new={number}:{start_sector}:{end_sector}"),
                        format!("--typecode={number}:{type_code}"),
                        format!("--change-name={number}:{label}"),
                        disk.clone(),
                    ],
                )),
        }
    }
    storage.push(run(Root::Live, "/usr/sbin/partprobe", [&disk]));
    storage.push(run(Root::Live, "/usr/bin/udevadm", ["settle"]));
    if matches!(plan.disk, DiskPlan::Whole { .. }) {
        storage.push(run(
            Root::Live,
            "/usr/sbin/mkfs.fat",
            ["-F", "32", &layout.esp.display().to_string()],
        ));
    }
    if let Some(boot) = &layout.boot {
        storage.push(if layout.boot_fs == "vfat" {
            run(Root::Live, "/usr/sbin/mkfs.fat", ["-F", "32", &boot.display().to_string()])
        } else {
            run(Root::Live, "/usr/sbin/mkfs.ext4", ["-F", &boot.display().to_string()])
        });
    }
    stages.push(Stage {
        title: if matches!(plan.disk, DiskPlan::Whole { .. }) {
            "Clearing the drive."
        } else {
            "Making space."
        }
        .into(),
        weight: 5,
        actions: storage,
    });
    let root = layout.root.display().to_string();
    let mut locking = vec![
        secret(
            "/usr/sbin/cryptsetup",
            &[
                "luksFormat",
                "--batch-mode",
                "--type",
                "luks2",
                "--pbkdf",
                "argon2id",
                "--cipher",
                "aes-xts-plain64",
                "--key-size",
                "512",
                "--label",
                "borea_root",
                "--key-file",
                "-",
                &root,
            ],
            SecretRef::Luks,
        ),
        secret(
            "/usr/sbin/cryptsetup",
            &["open", "--key-file", "-", &root, "${MAPPER_NAME}"],
            SecretRef::Luks,
        ),
        run(
            Root::Live,
            "/usr/sbin/mkfs.btrfs",
            ["-f", "-L", "borea_root", "/dev/mapper/${MAPPER_NAME}"],
        ),
        Action::Mount {
            source: "/dev/mapper/${MAPPER_NAME}".into(),
            path: TARGET.into(),
            fs: Some("btrfs".into()),
            options: "subvolid=5".into(),
        },
    ];
    for (sub, _) in &layout.subvolumes {
        if let Some(parent) =
            std::path::Path::new(sub).parent().filter(|p| !p.as_os_str().is_empty())
        {
            locking.push(run(
                Root::Live,
                "/usr/bin/mkdir",
                [
                    "--parents".into(),
                    "--mode=0755".into(),
                    format!("{TARGET}/{}", parent.display()),
                ],
            ));
        }
        locking.push(run(
            Root::Live,
            "/usr/bin/btrfs",
            ["subvolume".into(), "create".into(), format!("{TARGET}/{sub}")],
        ));
        let mountpoint = layout
            .subvolumes
            .iter()
            .find(|(name, _)| name == sub)
            .map(|(_, path)| path.as_str())
            .unwrap_or("/");
        let mode =
            if mountpoint == "/root" || mountpoint == "/.snapshots" { "0700" } else { "0755" };
        locking.push(run(Root::Live, "/usr/bin/chmod", [mode.into(), format!("{TARGET}/{sub}")]));
        locking.push(run(Root::Live, "/usr/bin/chown", ["0:0".into(), format!("{TARGET}/{sub}")]));
    }
    if plan.distro == Distro::OpenSuse {
        locking.push(capture(
            Root::Live,
            "/usr/bin/btrfs",
            &["inspect-internal", "rootid", &format!("{TARGET}/{}", layout.root_subvol)],
            "ROOT_SUBVOL_ID",
        ));
        locking.push(run(
            Root::Live,
            "/usr/bin/btrfs",
            ["subvolume", "set-default", "${ROOT_SUBVOL_ID}", TARGET],
        ));
    }
    locking.push(Action::Unmount { path: TARGET.into() });
    for (sub, path) in &layout.subvolumes {
        locking.push(Action::Mount {
            source: "/dev/mapper/${MAPPER_NAME}".into(),
            path: format!("{TARGET}{}", if path == "/" { "" } else { path }),
            fs: Some("btrfs".into()),
            options: format!("subvol={sub},compress=zstd:1,noatime"),
        });
    }
    if let Some(boot) = &layout.boot {
        locking.push(Action::Mount {
            source: boot.display().to_string(),
            path: format!("{TARGET}/boot"),
            fs: Some(layout.boot_fs.clone()),
            options: "defaults".into(),
        });
    }
    locking.push(Action::Mount {
        source: layout.esp.display().to_string(),
        path: format!("{TARGET}{}", layout.esp_mount),
        fs: Some("vfat".into()),
        options: "umask=0077".into(),
    });
    if matches!(plan.disk, DiskPlan::Alongside { .. }) {
        locking.push(Action::PreserveEfiFallback);
    }
    locking.push(capture(
        Root::Live,
        "/usr/sbin/blkid",
        &["-s", "UUID", "-o", "value", &root],
        "LUKS_UUID",
    ));
    locking.push(capture(
        Root::Live,
        "/usr/sbin/blkid",
        &["-s", "UUID", "-o", "value", "/dev/mapper/${MAPPER_NAME}"],
        "ROOT_UUID",
    ));
    stages.push(Stage { title: "Locking your space.".into(), weight: 3, actions: locking });
    let native = review
        .packages
        .iter()
        .filter(|p| p.source == base)
        .map(|p| p.name.clone())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    let cmdline = format!(
        "rd.luks.name=${{LUKS_UUID}}=borea-root root=/dev/mapper/borea-root rootflags=subvol={} {}",
        layout.root_subvol,
        review.kernel_args.join(" ")
    );
    let mut bootstrap_actions = match plan.distro {
        Distro::Debian => vec![run(
            Root::Live,
            "/usr/sbin/debootstrap",
            [
                "--variant=minbase",
                "--keyring=/usr/share/keyrings/debian-archive-keyring.gpg",
                "--include=ca-certificates",
                "trixie",
                TARGET,
                "https://deb.debian.org/debian",
            ],
        )],
        Distro::Arch => vec![run(
            Root::Tools,
            "/usr/bin/pacstrap",
            ["-K".into(), "/mnt".into()].into_iter().chain(native.clone()),
        )],
        Distro::Fedora => vec![run(
            Root::Tools,
            "/usr/bin/dnf",
            [
                "--installroot=/mnt".into(),
                format!("--releasever={}", plan.release),
                "--use-host-config".into(),
                "install".into(),
                "-y".into(),
                "@core".into(),
            ]
            .into_iter()
            .chain(native.clone()),
        )],
        Distro::OpenSuse => vec![run(
            Root::Tools,
            "/usr/bin/zypper",
            [
                "--root".into(),
                "/mnt".into(),
                "--non-interactive".into(),
                "install".into(),
                "--auto-agree-with-licenses".into(),
            ]
            .into_iter()
            .chain(native.clone()),
        )],
    };
    if plan.distro == Distro::OpenSuse {
        bootstrap_actions.insert(0, Action::CopyNativeRepositories);
        bootstrap_actions.insert(
            1,
            run(
                Root::Tools,
                "/usr/bin/zypper",
                [
                    "--root",
                    "/mnt",
                    "--gpg-auto-import-keys",
                    "--non-interactive",
                    "install",
                    "--auto-agree-with-licenses",
                    "--type",
                    "pattern",
                    "minimal_base",
                ],
            ),
        );
    }
    if plan.distro != Distro::Debian {
        bootstrap_actions.insert(0, write("/etc/kernel/cmdline", cmdline.clone() + "\n", 0o644));
    }
    if plan.distro == Distro::Fedora {
        bootstrap_actions.insert(
            0,
            write("/etc/kernel/install.conf", "layout=bls\nBOOT_ROOT=/boot\n".into(), 0o644),
        );
    }
    // Bind pseudo-filesystems only after a target root exists.
    for (src, path, fs, options) in [
        ("proc", "/proc", Some("proc"), "nosuid,nodev,noexec"),
        ("/sys", "/sys", None, "rbind,ro"),
        ("/dev", "/dev", None, "rbind"),
        ("tmpfs", "/run", Some("tmpfs"), "mode=0755,nosuid,nodev"),
    ] {
        bootstrap_actions.push(Action::Mount {
            source: src.into(),
            path: format!("{TARGET}{path}"),
            fs: fs.map(str::to_owned),
            options: options.into(),
        });
    }
    bootstrap_actions.push(Action::CopyNetwork);
    stages.push(Stage {
        title: format!("Installing {}.", plan.distro.name()),
        weight: 35,
        actions: bootstrap_actions,
    });
    let mut sources = BTreeMap::<String, Vec<String>>::new();
    for p in &review.packages {
        if p.source != base {
            sources.entry(p.source.clone()).or_default().push(p.name.clone());
        }
    }
    let mut repos = vec![Action::ImportRepositoryKeys];
    match plan.distro {
        Distro::Arch => {
            let multilib = sources.contains_key("arch.multilib")
                || plan.sources.get("arch.multilib") == Some(&true);
            repos.push(write("/etc/pacman.conf",format!("[options]\nArchitecture = auto\nCheckSpace\nSigLevel = Required DatabaseOptional\nLocalFileSigLevel = Optional\n[core]\nInclude = /etc/pacman.d/mirrorlist\n[extra]\nInclude = /etc/pacman.d/mirrorlist\n{}",if multilib{"[multilib]\nInclude = /etc/pacman.d/mirrorlist\n"}else{""}),0o644));
        }
        Distro::Debian => {
            let mut components = vec!["main"];
            for (id, name) in [
                ("debian.contrib", "contrib"),
                ("debian.non-free", "non-free"),
                ("debian.non-free-firmware", "non-free-firmware"),
            ] {
                if sources.contains_key(id) || plan.sources.get(id) == Some(&true) {
                    components.push(name);
                }
            }
            repos.push(write(
                "/etc/apt/sources.list.d/debian.sources",
                borea_backend::apt_sources(&components),
                0o644,
            ));
            if review.packages.iter().any(|package| {
                package.source.starts_with("debian.")
                    && (package.name.ends_with(":i386")
                        || borea_catalog::catalog::requires_debian_i386(&package.name))
            }) {
                repos.push(target("/usr/bin/dpkg", ["--add-architecture", "i386"]));
            }
            repos.push(target("/usr/bin/apt-get", ["update"]));
            repos.push(Action::DeferInitramfs);
            // Generate initramfs once, after UUIDs, encryption and keyboard are configured.
            repos.push(write(
                "/etc/initramfs-tools/update-initramfs.conf",
                "update_initramfs=no\nbackup_initramfs=no\n".into(),
                0o644,
            ));
            repos.push(target(
                "/usr/bin/apt-get",
                [
                    "-o".into(),
                    "Dpkg::Options::=--force-confold".into(),
                    "install".into(),
                    "-y".into(),
                    "--no-install-recommends".into(),
                ]
                .into_iter()
                .chain(native.clone()),
            ));
        }
        Distro::Fedora => {
            for (id, release) in
                [("fedora.rpmfusion-free", "free"), ("fedora.rpmfusion-nonfree", "nonfree")]
            {
                if sources.contains_key(id) || plan.sources.get(id) == Some(&true) {
                    repos.push(target("/usr/bin/dnf",["--setopt=localpkg_gpgcheck=1".into(),"install".into(),"-y".into(),format!("https://download1.rpmfusion.org/{release}/fedora/rpmfusion-{release}-release-{}.noarch.rpm",plan.release)]));
                }
            }
        }
        Distro::OpenSuse => {
            if sources.contains_key("opensuse.nvidia") {
                repos.push(target(
                    "/usr/bin/zypper",
                    ["--non-interactive", "install", "openSUSE-repos-NVIDIA"],
                ));
            }
            if sources.contains_key("opensuse.packman")
                || plan.sources.get("opensuse.packman") == Some(&true)
            {
                repos.push(target(
                    "/usr/bin/zypper",
                    [
                        "--non-interactive",
                        "addrepo",
                        "--refresh",
                        "--priority",
                        "90",
                        "https://ftp.gwdg.de/pub/linux/misc/packman/suse/openSUSE_Tumbleweed/",
                        "packman",
                    ],
                ));
            }
        }
    }
    for (src, pkgs) in &sources {
        if src.contains('.') {
            repos.push(install(plan.distro, pkgs.clone()));
        }
    }
    if !review.modprobe.is_empty() {
        repos.push(write("/etc/modprobe.d/borea.conf", review.modprobe.join("\n") + "\n", 0o644));
    }
    stages.push(Stage {
        title: "Setting up drivers and repositories.".into(),
        weight: 15,
        actions: repos,
    });
    let mut config = vec![
        Action::InstallHelpers,
        target("/usr/bin/systemctl", ["enable", "borea-firstboot.service"]),
        write("/etc/hostname", format!("{}\n", plan.user.hostname), 0o644),
        write(
            "/etc/hosts",
            format!(
                "127.0.0.1 localhost\n127.0.1.1 {}\n::1 localhost ip6-localhost ip6-loopback\n",
                plan.user.hostname
            ),
            0o644,
        ),
        write("/etc/locale.conf", format!("LANG={}\n", plan.locale.lang), 0o644),
        write(
            if plan.distro == Distro::Debian {
                "/etc/default/locale"
            } else {
                "/etc/vconsole.conf"
            },
            if plan.distro == Distro::Debian {
                format!("LANG={}\n", plan.locale.lang)
            } else {
                format!("KEYMAP={}\n", plan.locale.keyboard.console)
            },
            0o644,
        ),
        write(
            "/etc/X11/xorg.conf.d/00-keyboard.conf",
            format!(
                "Section \"InputClass\"\n Identifier \"borea-keyboard\"\n MatchIsKeyboard \"on\"\n Option \"XkbLayout\" \"{}\"\n Option \"XkbVariant\" \"{}\"\nEndSection\n",
                plan.locale.keyboard.layout, plan.locale.keyboard.variant
            ),
            0o644,
        ),
        target(
            "/usr/bin/ln",
            [
                "-sf".into(),
                format!("/usr/share/zoneinfo/{}", plan.locale.timezone),
                "/etc/localtime".into(),
            ],
        ),
        write(
            "/etc/systemd/zram-generator.conf",
            "[zram0]\nzram-size = min(ram / 2, 8192)\ncompression-algorithm = zstd\n".into(),
            0o644,
        ),
        Action::ConfigureFstab,
    ];
    if plan.distro == Distro::Debian {
        config.push(write(
            "/etc/default/keyboard",
            format!(
                "XKBMODEL=pc105\nXKBLAYOUT={}\nXKBVARIANT={}\nXKBOPTIONS=\nBACKSPACE=guess\n",
                plan.locale.keyboard.layout, plan.locale.keyboard.variant
            ),
            0o644,
        ));
        config.push(target("/usr/bin/setupcon", ["--save-only"]));
    }
    if matches!(plan.distro, Distro::Arch | Distro::Debian) {
        config.push(write("/etc/locale.gen", format!("{} UTF-8\n", plan.locale.lang), 0o644));
        config.push(target("/usr/bin/locale-gen", Vec::<String>::new()));
    }
    if review.services.iter().any(|service| service == "ssh.service" || service == "sshd.service") {
        config.push(write(
            "/etc/ssh/sshd_config.d/90-borea-root.conf",
            "PermitRootLogin no\n".into(),
            0o644,
        ));
    }
    if plan.distro == Distro::Arch {
        if review.services.iter().any(|service| service == "postgresql.service") {
            config.push(target(
                "/usr/sbin/runuser",
                ["-u", "postgres", "--", "/usr/bin/initdb", "-D", "/var/lib/postgres/data"],
            ));
        }
        if review.services.iter().any(|service| service == "mariadb.service") {
            config.push(target(
                "/usr/bin/mariadb-install-db",
                ["--user=mysql", "--basedir=/usr", "--datadir=/var/lib/mysql"],
            ));
        }
    }
    if plan.distro == Distro::Fedora
        && review.services.iter().any(|service| service == "postgresql.service")
    {
        config.push(target("/usr/bin/postgresql-setup", ["--initdb", "--unit", "postgresql"]));
    }
    if plan.distro == Distro::OpenSuse {
        config.push(write("/etc/snapper/configs/root", "SUBVOLUME=\"/\"\nFSTYPE=\"btrfs\"\nTIMELINE_CREATE=\"yes\"\nTIMELINE_CLEANUP=\"yes\"\nNUMBER_CLEANUP=\"yes\"\nNUMBER_LIMIT=\"50\"\n".into(),0o640));
        config.push(write("/etc/sysconfig/snapper", "SNAPPER_CONFIGS=\"root\"\n".into(), 0o644));
        config.push(target(
            "/usr/bin/systemctl",
            ["enable", "snapper-timeline.timer", "snapper-cleanup.timer"],
        ));
    }
    let admin = borea_backend::admin_group(plan.distro);
    config.push(target("/usr/sbin/groupadd", ["--system", "--force", admin]));
    config.push(target(
        "/usr/sbin/useradd",
        [
            "--create-home".into(),
            "--user-group".into(),
            "--shell".into(),
            "/bin/bash".into(),
            "--groups".into(),
            admin.into(),
            "--comment".into(),
            plan.user.full_name.clone(),
            plan.user.username.clone(),
        ],
    ));
    let mut password = target("/usr/sbin/chpasswd", Vec::<String>::new());
    if let Action::Run(c) = &mut password {
        c.stdin = Some(SecretRef::Login);
        c.retry = false;
    }
    config.push(password);
    let mut rootpassword = target("/usr/sbin/chpasswd", Vec::<String>::new());
    if let Action::Run(c) = &mut rootpassword {
        c.stdin = Some(SecretRef::RootPassword);
        c.retry = false;
    }
    config.push(rootpassword);
    config.push(write(
        "/etc/sudoers.d/10-borea",
        format!(
            "%{admin} ALL=(ALL:ALL) ALL\n{}",
            if plan.user.same_passphrase { "" } else { "Defaults rootpw\n" }
        ),
        0o440,
    ));
    config.push(target("/usr/sbin/visudo", ["-cf", "/etc/sudoers.d/10-borea"]));
    for service in review.services.iter().chain(std::iter::once(&"fstrim.timer".into())) {
        config.push(target("/usr/bin/systemctl", ["enable", service]));
    }
    config.push(target(
        "/usr/bin/systemctl",
        [
            "set-default",
            if plan.desktop.is_some() { "graphical.target" } else { "multi-user.target" },
        ],
    ));
    for rule in &review.firewall {
        config.push(target(
            "/usr/bin/firewall-offline-cmd",
            [if rule.contains('/') {
                format!("--add-port={rule}")
            } else {
                format!("--add-service={rule}")
            }],
        ));
    }
    stages.push(Stage {
        title: "Configuring your system and account.".into(),
        weight: 20,
        actions: config,
    });
    let mut apps = Vec::new();
    if let Some(pkgs) = sources.get("aur") {
        apps.push(Action::BuildAur { packages: pkgs.clone() });
    }
    if let Some(pkgs) = sources.get("flathub") {
        apps.push(install(plan.distro, vec!["flatpak".into()]));
        apps.push(target(
            "/usr/bin/flatpak",
            [
                "remote-add",
                "--system",
                "--if-not-exists",
                "flathub",
                "https://dl.flathub.org/repo/flathub.flatpakrepo",
            ],
        ));
        apps.push(target(
            "/usr/bin/flatpak",
            [
                "install".into(),
                "--system".into(),
                "--noninteractive".into(),
                "-y".into(),
                "flathub".into(),
            ]
            .into_iter()
            .chain(pkgs.clone()),
        ));
    }
    if let Some(releases) = sources.get("appimage") {
        apps.push(Action::InstallAppImages { releases: releases.clone() });
    }
    stages.push(Stage { title: "Adding your apps.".into(), weight: 15, actions: apps });
    let mut boot = vec![write("/etc/kernel/cmdline", cmdline.clone() + "\n", 0o644)];
    match plan.distro {
        Distro::Arch => {
            boot.push(write("/etc/mkinitcpio.conf","MODULES=()\nBINARIES=()\nFILES=()\nHOOKS=(base systemd autodetect microcode modconf kms keyboard sd-vconsole block sd-encrypt filesystems fsck)\n".into(),0o644));
            boot.push(target("/usr/bin/mkinitcpio", ["-P"]));
            let mut arguments = vec![format!("--esp-path={}", layout.esp_mount)];
            if layout.boot.is_some() {
                arguments.push("--boot-path=/boot".into());
            }
            arguments.push("install".into());
            boot.push(target("/usr/bin/bootctl", arguments));
            boot.push(write(
                &format!("{}/loader/loader.conf", layout.esp_mount),
                "default arch.conf\ntimeout 4\neditor no\n".into(),
                0o644,
            ));
            boot.push(write("/boot/loader/entries/arch.conf",format!("title Arch Linux\nlinux /vmlinuz-linux\ninitrd /initramfs-linux.img\noptions {cmdline}\n"),0o644));
        }
        Distro::Debian => {
            boot.push(write("/etc/default/grub",format!("GRUB_DEFAULT=0\nGRUB_TIMEOUT=4\nGRUB_CMDLINE_LINUX=\"root=/dev/mapper/borea-root rootflags=subvol={} {}\"\nGRUB_DISABLE_OS_PROBER=false\n",layout.root_subvol,review.kernel_args.join(" ")),0o644));
            boot.push(Action::RestoreInitramfs);
            boot.push(write(
                "/etc/initramfs-tools/update-initramfs.conf",
                "update_initramfs=yes\nbackup_initramfs=no\n".into(),
                0o644,
            ));
            boot.push(target("/usr/sbin/update-initramfs", ["-c", "-k", "all"]));
            boot.push(target(
                "/usr/bin/mkdir",
                ["-p", "/boot/grub", "/boot/efi/EFI/debian", "/boot/efi/EFI/BOOT"],
            ));
            boot.push(target(
                "/usr/bin/cp",
                ["/usr/lib/shim/shimx64.efi.signed", "/boot/efi/EFI/debian/shimx64.efi"],
            ));
            boot.push(target(
                "/usr/bin/cp",
                ["/usr/lib/shim/mmx64.efi.signed", "/boot/efi/EFI/debian/mmx64.efi"],
            ));
            boot.push(target(
                "/usr/bin/cp",
                [
                    "/usr/lib/grub/x86_64-efi-signed/grubx64.efi.signed",
                    "/boot/efi/EFI/debian/grubx64.efi",
                ],
            ));
            boot.push(write("/boot/efi/EFI/debian/grub.cfg","search --no-floppy --fs-uuid --set=root ${BOOT_UUID}\nset prefix=($root)/grub\nconfigfile $prefix/grub.cfg\n".into(),0o644));
            boot.push(Action::InstallEfiFallback {
                shim: "/boot/efi/EFI/debian/shimx64.efi".into(),
                grub: "/boot/efi/EFI/debian/grubx64.efi".into(),
                mok: "/boot/efi/EFI/debian/mmx64.efi".into(),
                config: "search --no-floppy --fs-uuid --set=root ${BOOT_UUID}\nset prefix=($root)/grub\nconfigfile $prefix/grub.cfg\n".into(),
            });
            boot.push(run(
                Root::Live,
                "/usr/sbin/efibootmgr",
                [
                    "--create".into(),
                    "--disk".into(),
                    layout.disk.path.display().to_string(),
                    "--part".into(),
                    layout.esp_number.to_string(),
                    "--label".into(),
                    "Debian".into(),
                    "--loader".into(),
                    "\\EFI\\debian\\shimx64.efi".into(),
                ],
            ));
            boot.push(target("/usr/sbin/update-grub", Vec::<String>::new()));
        }
        Distro::Fedora => {
            boot.push(write("/etc/default/grub", format!("GRUB_DEFAULT=saved\nGRUB_TIMEOUT=4\nGRUB_ENABLE_BLSCFG=true\nGRUB_DISABLE_OS_PROBER=false\nGRUB_CMDLINE_LINUX=\"{cmdline}\"\n"), 0o644));
            boot.push(target("/usr/bin/mkdir", ["-p", "/boot/loader/entries", "/boot/grub2"]));
            boot.push(write(
                "/etc/kernel/install.conf",
                "layout=bls\nBOOT_ROOT=/boot\n".into(),
                0o644,
            ));
            boot.push(target("/usr/bin/kernel-install", ["add-all"]));
            boot.push(target(
                "/usr/sbin/grubby",
                [
                    "--update-kernel=ALL".into(),
                    "--remove-args=root rootflags rd.luks.uuid rd.luks.name".into(),
                    format!("--args={cmdline}"),
                ],
            ));
            boot.push(target(
                "/usr/bin/grub2-editenv",
                ["-".into(), "set".into(), format!("kernelopts={cmdline}")],
            ));
            boot.push(target("/usr/bin/dracut", ["--regenerate-all", "--force"]));
            boot.push(target("/usr/sbin/grub2-mkconfig", ["-o", "/boot/grub2/grub.cfg"]));
            boot.push(write("/boot/efi/EFI/fedora/grub.cfg", "search --no-floppy --fs-uuid --set=root ${BOOT_UUID}\nset prefix=($root)/grub2\nconfigfile $prefix/grub.cfg\n".into(), 0o644));
            boot.push(write("/.autorelabel", "-F\n".into(), 0o600));
            boot.push(run(
                Root::Live,
                "/usr/sbin/efibootmgr",
                [
                    "--create".into(),
                    "--disk".into(),
                    layout.disk.path.display().to_string(),
                    "--part".into(),
                    layout.esp_number.to_string(),
                    "--label".into(),
                    "Fedora".into(),
                    "--loader".into(),
                    "\\EFI\\fedora\\shimx64.efi".into(),
                ],
            ));
        }
        Distro::OpenSuse => {
            boot.push(target("/usr/bin/dracut", ["--regenerate-all", "--force"]));
            boot.push(write(
                "/etc/sysconfig/bootloader",
                format!(
                    "LOADER_TYPE=\"systemd-boot\"\nSECURE_BOOT=\"{}\"\nUPDATE_NVRAM=\"yes\"\n",
                    if secure_boot { "yes" } else { "no" }
                ),
                0o644,
            ));
            let mut install =
                vec!["--esp-path".into(), layout.esp_mount.clone(), "--arch".into(), "x64".into()];
            if secure_boot {
                install.push("--secure-boot".into());
            }
            install.push("install".into());
            boot.push(target("/usr/bin/sdbootutil", install));
            boot.push(target(
                "/usr/bin/sdbootutil",
                ["--esp-path", layout.esp_mount.as_str(), "--arch", "x64", "add-all-kernels"],
            ));
        }
    }
    if secure_boot && plan.distro == Distro::Arch {
        boot.push(install(plan.distro, vec!["sbctl".into(), "sbsigntools".into()]));
        boot.push(target("/usr/bin/sbctl", ["create-keys"]));
        boot.push(write("/etc/mkinitcpio.d/linux.preset","ALL_config=\"/etc/mkinitcpio.conf\"\nALL_kver=\"/boot/vmlinuz-linux\"\nPRESETS=('default')\ndefault_uki=\"/boot/EFI/Linux/arch-linux.efi\"\n".into(),0o644));
        boot.push(target("/usr/bin/mkdir", ["-p", "/boot/EFI/Linux"]));
        boot.push(target("/usr/bin/mkinitcpio", ["-P"]));
        for file in [
            format!("{}/EFI/systemd/systemd-bootx64.efi", layout.esp_mount),
            "/boot/EFI/Linux/arch-linux.efi".into(),
        ] {
            boot.push(target("/usr/bin/sbctl", ["sign".into(), "--save".into(), file]));
        }
        boot.push(Action::SignEfiFallback);
        boot.push(target("/usr/bin/sbctl", ["enroll-keys", "-m"]));
        boot.push(write(
            "/boot/loader/entries/arch.conf",
            "title Arch Linux\nefi /EFI/Linux/arch-linux.efi\n".into(),
            0o644,
        ));
        boot.push(write("/etc/pacman.d/hooks/zz-borea-secureboot.hook","[Trigger]\nOperation = Install\nOperation = Upgrade\nType = Package\nTarget = linux\nTarget = systemd\n[Action]\nDescription = Renew Borea Secure Boot signatures\nWhen = PostTransaction\nExec = /usr/bin/sbctl sign-all\n".into(),0o644));
    }
    if plan.distro == Distro::Fedora
        && review.packages.iter().any(|p| p.name.starts_with("akmod-nvidia"))
    {
        boot.push(capture(
            Root::Target,
            "/usr/bin/find",
            &["/usr/lib/modules", "-mindepth", "1", "-maxdepth", "1", "-printf", "%f\n"],
            "KERNEL_VERSION",
        ));
        boot.push(target("/usr/sbin/akmods", ["--force", "--kernels", "${KERNEL_VERSION}"]));
        boot.push(target("/usr/bin/dracut", ["--regenerate-all", "--force"]));
    }
    if review.mok && plan.distro != Distro::Arch {
        boot.push(install(plan.distro, vec!["mokutil".into()]));
        boot.push(Action::EnrollMok);
    }
    if matches!(plan.disk, DiskPlan::Alongside { .. }) {
        boot.push(Action::RestoreEfiFallback);
    }
    if plan.distro == Distro::OpenSuse {
        boot.push(Action::RestoreTransportTrust);
    }
    boot.push(Action::RestoreNetwork);
    boot.push(run(Root::Live, "/usr/bin/sync", Vec::<String>::new()));
    stages.push(Stage { title: "Finishing up.".into(), weight: 7, actions: boot });
    Ok(Job {
        secure_boot,
        firmware_secure_boot: hw.secure_boot,
        setup_mode: hw.setup_mode,
        external: borea_backend::external::ExternalSources::default(),
        version: 1,
        plan: plan.clone(),
        review,
        layout,
        bootstrap: bootstrap.clone(),
        stages,
    })
}

/// Pin external source bytes before returning the approval digest to the UI.
pub fn apply_external(job: &mut Job, resolved: borea_backend::external::ExternalSources) {
    for image in &resolved.appimages {
        if !image.published_checksum {
            job.review.warnings.push(format!("{} {}: upstream supplies no published checksum; downloaded bytes are pinned but provenance is unverified.",image.repository,image.tag));
        }
    }
    if let Some(base) = borea_catalog::repos_for(job.plan.distro).find(|repo| repo.base) {
        for recipe in &resolved.aur {
            for name in &recipe.dependencies {
                if !job.review.packages.iter().any(|p| p.name == *name && p.source == base.id) {
                    job.review
                        .packages
                        .push(borea_core::Package { source: base.id.into(), name: name.clone() });
                }
            }
        }
        job.review.packages.sort_by(|a, b| (&a.source, &a.name).cmp(&(&b.source, &b.name)));
    }
    job.external = resolved;
}
pub fn resolve_external(job: &mut Job) -> Result<(), Hint> {
    let mut resolved = borea_backend::external::resolve(&job.review)?;
    let mut sources = job.review.packages.iter().map(|p| p.source.clone()).collect::<BTreeSet<_>>();
    sources.extend(job.plan.sources.iter().filter(|(_, on)| **on).map(|(key, _)| key.clone()));
    for source in sources {
        if let Some(key) = borea_backend::external::repository_key(&source, &job.plan.release)? {
            resolved.repository_keys.push(key);
        }
    }
    apply_external(job, resolved);
    Ok(())
}
