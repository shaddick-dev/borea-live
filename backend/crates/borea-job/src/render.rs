//! Human review of executor-owned operations, including conditional native helpers.
use crate::{Action, Job, Root, TARGET, quote};
use borea_core::Distribution as D;
fn command(out: &mut String, job: &Job, root: Root, program: &str, args: Vec<String>) {
    let prefix = match root {
        Root::Live => String::new(),
        Root::Target => format!("chroot {} ", quote(TARGET)),
        Root::Tools => {
            format!("chroot {} ", quote(&format!("/run/borea/tools/{}", job.plan.distro.key())))
        }
    };
    out.push_str(&format!(
        "{prefix}{} {}\n",
        quote(program),
        args.iter().map(|s| quote(s)).collect::<Vec<_>>().join(" ")
    ));
}
fn args(values: &[&str]) -> Vec<String> {
    values.iter().map(|s| (*s).into()).collect()
}
fn note(out: &mut String, text: &str) {
    out.push_str(&format!("# {}\n", text.replace('\n', "\n# ")));
}
pub(crate) fn helper(job: &Job, action: &Action, out: &mut String) {
    match action {
        Action::Mount { source, path, fs, options } => {
            let mut args = Vec::new();
            if let Some(fs) = fs {
                args.extend(["-t".into(), fs.clone()]);
            }
            args.extend(["-o".into(), options.clone(), source.clone(), path.clone()]);
            command(out, job, Root::Live, "mount", args);
        }
        Action::Unmount { path } => command(out, job, Root::Live, "umount", vec![path.clone()]),
        Action::AcquireToolRoot { artifact } => {
            let root = format!("/run/borea/tools/{}", job.plan.distro.key());
            note(
                out,
                &format!(
                    "HTTPS GET {} -> {root}.tar; verify SHA256 {} before unpacking.",
                    artifact.url, artifact.sha256
                ),
            );
            note(out, &format!("HTTPS GET {} -> {root}.sig", artifact.signature_url));
            if let Some(proof) = &artifact.checksum {
                note(
                    out,
                    &format!(
                        "Fetch signed checksum metadata {} and require entry {} with the approved hash.",
                        proof.url, proof.filename
                    ),
                );
                let mut args = vec!["--keyring".into(), artifact.keyring.clone()];
                if proof.url == artifact.signature_url {
                    args.extend([
                        "--output".into(),
                        format!("{root}.verified-checksums"),
                        format!("{root}.checksums"),
                    ]);
                } else {
                    args.extend([format!("{root}.sig"), format!("{root}.checksums")]);
                }
                command(out, job, Root::Live, "/usr/bin/gpgv", args);
            } else {
                command(
                    out,
                    job,
                    Root::Live,
                    "/usr/bin/gpgv",
                    vec![
                        "--keyring".into(),
                        artifact.keyring.clone(),
                        format!("{root}.sig"),
                        format!("{root}.tar"),
                    ],
                );
            }
            command(
                out,
                job,
                Root::Live,
                "/usr/bin/tar",
                vec!["-tf".into(), format!("{root}.tar")],
            );
            note(
                out,
                "Archive format is detected only after authentication; the matching branch runs:",
            );
            command(
                out,
                job,
                Root::Live,
                "/usr/bin/tar",
                vec![
                    "--extract".into(),
                    "--file".into(),
                    format!("{root}.tar"),
                    "--directory".into(),
                    root.clone(),
                    format!(
                        "--strip-components={}",
                        if job.plan.distro == D::Arch { 1 } else { 0 }
                    ),
                ],
            );
            command(
                out,
                job,
                Root::Live,
                "/usr/bin/skopeo",
                vec![
                    "copy".into(),
                    format!("<docker-archive-or-oci-archive>:{root}.tar"),
                    format!("oci:{root}.oci:borea"),
                ],
            );
            command(
                out,
                job,
                Root::Live,
                "/usr/bin/umoci",
                vec![
                    "unpack".into(),
                    "--image".into(),
                    format!("{root}.oci:borea"),
                    format!("{root}.bundle"),
                ],
            );
            note(
                out,
                "For container archives move the verified bundle/rootfs into the tool root. Mount proc, sys and dev there; native tooling must exist. Temporary session CA trust is copied into /run.",
            );
        }
        Action::PrepareExternal => {
            note(
                out,
                "Check live utilities and the static x86-64 target helper before disk changes.",
            );
            for key in &job.external.repository_keys {
                note(
                    out,
                    &format!(
                        "HTTPS GET {} (SHA256 {}) -> /run/borea/external/{}.asc",
                        key.url, key.sha256, key.source
                    ),
                );
            }
            for recipe in &job.external.aur {
                note(
                    out,
                    &format!(
                        "HTTPS GET {} (SHA256 {}) -> /run/borea/external/{}.tar.gz",
                        recipe.url, recipe.sha256, recipe.base
                    ),
                );
            }
            for image in &job.external.appimages {
                note(
                    out,
                    &format!(
                        "HTTPS GET {} (SHA256 {}) -> /run/borea/external/{}",
                        image.url, image.sha256, image.filename
                    ),
                );
            }
        }
        Action::PreflightNative => {
            for key in &job.external.repository_keys {
                command(
                    out,
                    job,
                    Root::Tools,
                    "/usr/bin/rpm",
                    vec!["--import".into(), format!("/etc/pki/rpm-gpg/BOREA-{}", key.source)],
                );
            }
            match job.plan.distro {
                D::Arch => {
                    note(
                        out,
                        "Write the HTTPS Arch mirrorlist and enable multilib only when the reviewed selection requires it.",
                    );
                    command(out, job, Root::Tools, "/usr/bin/pacman-key", args(&["--init"]));
                    command(
                        out,
                        job,
                        Root::Tools,
                        "/usr/bin/pacman-key",
                        args(&["--populate", "archlinux"]),
                    );
                    command(
                        out,
                        job,
                        Root::Tools,
                        "/usr/bin/pacman",
                        args(&["-Sy", "--noconfirm"]),
                    );
                    command(
                        out,
                        job,
                        Root::Tools,
                        "/usr/bin/pacman",
                        args(&["-Sp", "--noconfirm"])
                            .into_iter()
                            .chain(
                                job.review
                                    .packages
                                    .iter()
                                    .filter(|p| p.source.contains('.'))
                                    .map(|p| p.name.clone()),
                            )
                            .collect(),
                    );
                }
                D::Fedora => {
                    for kind in ["free", "nonfree"] {
                        let source = format!("fedora.rpmfusion-{kind}");
                        if job.review.packages.iter().any(|p| p.source == source)
                            || job.plan.sources.get(&source) == Some(&true)
                        {
                            command(
                                out,
                                job,
                                Root::Tools,
                                "/usr/bin/dnf",
                                vec![
                                    "--setopt=localpkg_gpgcheck=1".into(),
                                    "install".into(),
                                    "-y".into(),
                                    format!(
                                        "https://download1.rpmfusion.org/{kind}/fedora/rpmfusion-{kind}-release-{}.noarch.rpm",
                                        job.plan.release
                                    ),
                                ],
                            );
                        }
                    }
                    command(
                        out,
                        job,
                        Root::Tools,
                        "/usr/bin/dnf",
                        args(&["makecache", "--refresh"]),
                    );
                    command(
                        out,
                        job,
                        Root::Tools,
                        "/usr/bin/dnf",
                        args(&["repoquery", "--available", "--queryformat=%{name}"])
                            .into_iter()
                            .chain(
                                job.review
                                    .packages
                                    .iter()
                                    .filter(|p| p.source.contains('.') && !p.name.starts_with('@'))
                                    .map(|p| p.name.clone()),
                            )
                            .collect(),
                    );
                    for package in job.review.packages.iter().filter(|p| p.name.starts_with('@')) {
                        command(
                            out,
                            job,
                            Root::Tools,
                            "/usr/bin/dnf",
                            vec![
                                "environment".into(),
                                "info".into(),
                                package.name.trim_start_matches('@').trim_start_matches('^').into(),
                            ],
                        );
                    }
                }
                D::OpenSuse => {
                    command(
                        out,
                        job,
                        Root::Tools,
                        "/usr/bin/zypper",
                        args(&["--gpg-auto-import-keys", "--non-interactive", "refresh"]),
                    );
                    if job.review.packages.iter().any(|p| p.source == "opensuse.nvidia") {
                        command(
                            out,
                            job,
                            Root::Tools,
                            "/usr/bin/zypper",
                            args(&["--non-interactive", "install", "openSUSE-repos-NVIDIA"]),
                        );
                    }
                    if job.review.packages.iter().any(|p| p.source == "opensuse.packman")
                        || job.plan.sources.get("opensuse.packman") == Some(&true)
                    {
                        command(
                            out,
                            job,
                            Root::Tools,
                            "/usr/bin/zypper",
                            args(&[
                                "--non-interactive",
                                "addrepo",
                                "--refresh",
                                "--priority",
                                "90",
                                "https://ftp.gwdg.de/pub/linux/misc/packman/suse/openSUSE_Tumbleweed/",
                                "packman",
                            ]),
                        );
                    }
                    command(
                        out,
                        job,
                        Root::Tools,
                        "/usr/bin/zypper",
                        args(&["--non-interactive", "refresh"]),
                    );
                    command(
                        out,
                        job,
                        Root::Tools,
                        "/usr/bin/zypper",
                        args(&[
                            "--non-interactive",
                            "install",
                            "--dry-run",
                            "--auto-agree-with-licenses",
                        ])
                        .into_iter()
                        .chain(
                            job.review
                                .packages
                                .iter()
                                .filter(|p| p.source.contains('.'))
                                .map(|p| p.name.clone()),
                        )
                        .collect(),
                    );
                }
                D::Debian => {}
            }
        }
        Action::ImportRepositoryKeys => {
            for key in &job.external.repository_keys {
                command(
                    out,
                    job,
                    Root::Target,
                    "/usr/bin/rpm",
                    vec!["--import".into(), format!("/etc/pki/rpm-gpg/BOREA-{}", key.source)],
                );
            }
        }
        Action::CopyNativeRepositories => note(
            out,
            "Copy native openSUSE repository definitions from the authenticated tool root into the target.",
        ),
        Action::DeferInitramfs | Action::RestoreInitramfs => {
            let defer = matches!(action, Action::DeferInitramfs);
            command(
                out,
                job,
                Root::Target,
                "/usr/bin/dpkg-divert",
                args(&[
                    if defer { "--add" } else { "--remove" },
                    "--rename",
                    "--divert",
                    "/usr/sbin/update-initramfs.borea-real",
                    "/usr/sbin/update-initramfs",
                ]),
            );
            note(
                out,
                if defer {
                    "Temporarily install the static Rust initramfs guard; final boot configuration restores the native tool."
                } else {
                    "Remove the temporary guard and restore the native initramfs tool before building boot images."
                },
            );
        }
        Action::ConfigureFstab => {
            for device in std::iter::once(&job.layout.esp).chain(job.layout.boot.iter()) {
                command(
                    out,
                    job,
                    Root::Live,
                    "/usr/sbin/blkid",
                    vec![
                        "-s".into(),
                        "UUID".into(),
                        "-o".into(),
                        "value".into(),
                        device.display().to_string(),
                    ],
                );
            }
            note(
                out,
                "Write fstab and crypttab from discovered filesystem/LUKS UUIDs and the reviewed btrfs layout.",
            );
        }
        Action::RestoreTransportTrust => note(
            out,
            "Unmount the temporary hashed transport CA directory and regenerate the installed system's native certificate cache. Session proxy trust is never persisted.",
        ),
        Action::RestoreNetwork => note(
            out,
            "Restore the target's native DNS file/symlink and refresh the selected Wi-Fi profile before unmounting.",
        ),
        Action::CopyNetwork => note(
            out,
            "Copy only the active NetworkManager profile identified by its UUID, root-owned mode 0600; preserve credentials verbatim. Copy DNS configuration for the native chroot.",
        ),
        Action::BuildAur { .. } => {
            command(
                out,
                job,
                Root::Target,
                "/usr/bin/pacman",
                args(&["-S", "--needed", "--noconfirm", "base-devel", "git"]),
            );
            command(
                out,
                job,
                Root::Target,
                "/usr/bin/useradd",
                args(&[
                    "--system",
                    "--user-group",
                    "--create-home",
                    "--home-dir",
                    "/var/lib/borea-build",
                    "borea-build",
                ]),
            );
            for recipe in &job.external.aur {
                let directory = format!("/var/lib/borea-build/{}", recipe.base);
                if !recipe.dependencies.is_empty() {
                    command(
                        out,
                        job,
                        Root::Target,
                        "/usr/bin/pacman",
                        args(&["-S", "--needed", "--noconfirm"])
                            .into_iter()
                            .chain(recipe.dependencies.clone())
                            .collect(),
                    );
                }
                command(
                    out,
                    job,
                    Root::Target,
                    "/usr/bin/chown",
                    vec!["-R".into(), "borea-build:borea-build".into(), directory.clone()],
                );
                command(
                    out,
                    job,
                    Root::Target,
                    "/usr/sbin/runuser",
                    args(&["-u", "borea-build", "--", "/usr/bin/tar", "-xzf"])
                        .into_iter()
                        .chain([
                            format!("{directory}/recipe.tar.gz"),
                            "--strip-components=1".into(),
                            "-C".into(),
                            directory.clone(),
                            "--no-same-owner".into(),
                        ])
                        .collect(),
                );
                command(
                    out,
                    job,
                    Root::Target,
                    "/usr/sbin/runuser",
                    args(&["-u", "borea-build", "--", "/usr/bin/env"])
                        .into_iter()
                        .chain([
                            format!("--chdir={directory}"),
                            "/usr/bin/makepkg".into(),
                            "--cleanbuild".into(),
                            "--noconfirm".into(),
                        ])
                        .collect(),
                );
                command(
                    out,
                    job,
                    Root::Target,
                    "/usr/bin/pacman",
                    args(&["-Qp", "<built-package-artifact>"]),
                );
                command(
                    out,
                    job,
                    Root::Target,
                    "/usr/bin/pacman",
                    args(&[
                        "-U",
                        "--needed",
                        "--noconfirm",
                        "<artifacts-matching-reviewed-package-names>",
                    ]),
                );
            }
            command(
                out,
                job,
                Root::Target,
                "/usr/sbin/runuser",
                args(&[
                    "-u",
                    "borea-build",
                    "--",
                    "/usr/bin/gpgconf",
                    "--homedir",
                    "/var/lib/borea-build/.gnupg",
                    "--kill",
                    "all",
                ]),
            );
            command(out, job, Root::Target, "/usr/bin/userdel", args(&["--remove", "borea-build"]));
        }
        Action::InstallAppImages { releases } => {
            for image in
                job.external.appimages.iter().filter(|image| releases.contains(&image.repository))
            {
                let directory =
                    format!("/opt/borea/appimages/{}", image.repository.replace('/', "-"));
                let owner = format!("{}:{}", job.plan.user.username, job.plan.user.username);
                command(
                    out,
                    job,
                    Root::Target,
                    "/usr/bin/chown",
                    vec!["-R".into(), owner, directory.clone()],
                );
                command(
                    out,
                    job,
                    Root::Target,
                    "/usr/sbin/runuser",
                    vec![
                        "-u".into(),
                        job.plan.user.username.clone(),
                        "--".into(),
                        "/usr/bin/borea-firstboot".into(),
                        "--extract-appimage".into(),
                        directory,
                        image.filename.clone(),
                    ],
                );
                note(
                    out,
                    "Install a desktop entry using extracted AppRun/.DirIcon and a root-owned upstream update record.",
                );
            }
            if !job.external.appimages.is_empty() {
                command(
                    out,
                    job,
                    Root::Target,
                    "/usr/bin/systemctl",
                    args(&["enable", "borea-appimage-update.timer"]),
                );
            }
        }
        Action::EnrollMok => {
            note(
                out,
                "stdin: <redacted secret>, twice. Enrollment is confirmed in firmware on first restart.",
            );
            command(
                out,
                job,
                Root::Target,
                "/usr/bin/mokutil",
                args(&[
                    "--import",
                    match job.plan.distro {
                        D::Fedora => "/etc/pki/akmods/certs/public_key.der",
                        D::Debian => "/var/lib/dkms/mok.pub",
                        _ => "<native-NVIDIA-MOK-certificates>",
                    },
                ]),
            );
        }
        Action::InstallHelpers => note(
            out,
            "Install the static Rust first-boot/updater helper, an empty root-owned queue, and its systemd units.",
        ),
        Action::InstallEfiFallback { shim, grub, mok, .. } => note(
            out,
            &format!(
                "Only when EFI/BOOT contains none of BOOTX64.EFI, grubx64.efi, mmx64.efi or grub.cfg: copy {shim}, {grub}, {mok} and the UUID-based loader configuration. Preserve existing fallback files."
            ),
        ),
        Action::PreserveEfiFallback => note(
            out,
            "Keep existing EFI/BOOT files in bounded RAM before native package/bootloader actions; reject symlinks and oversized trees.",
        ),
        Action::RestoreEfiFallback => note(
            out,
            "Restore every existing EFI/BOOT file verbatim before unmounting, also on failure.",
        ),
        Action::SignEfiFallback => {
            note(
                out,
                "Only when no pre-existing fallback BOOTX64.EFI was saved: sign and track the newly installed Arch fallback. Existing fallback binaries are never signed or tracked.",
            );
            command(
                out,
                job,
                Root::Target,
                "/usr/bin/sbctl",
                vec![
                    "sign".into(),
                    "--save".into(),
                    format!("{}/EFI/BOOT/BOOTX64.EFI", job.layout.esp_mount),
                ],
            );
        }
        Action::Note { message } => note(out, message),
        Action::Run(_) | Action::WriteFile { .. } => {}
    }
}
