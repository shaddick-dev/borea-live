//! How to start an installer ISO that sits as a file on a partition.
//!
//! Arch-based ISOs (Omarchy's among them) ship a GRUB `loopback.cfg` whose menu
//! entries boot the ISO from a file: `img_dev=UUID=… img_loop=…`. Borea reads the
//! installer's own entry from the downloaded ISO, rather than keeping a copy of its
//! kernel options, so the hand-off follows whatever that release needs.

/// What `kexec` loads: paths inside the ISO, and the kernel command line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BootSpec {
    pub kernel: String,
    pub initrds: Vec<String>,
    pub cmdline: String,
}

/// Where the ISO file is, for the installer's own initramfs to find it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IsoFile<'a> {
    /// The filesystem UUID of the partition holding the ISO.
    pub uuid: &'a str,
    /// The ISO's path on that partition, starting with `/`.
    pub path: &'a str,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BootError {
    NoEntry(String),
    NoKernel,
    NoInitrd,
    UnknownVariable(String),
    Unsafe(String),
    NotLoopback,
}

impl std::fmt::Display for BootError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BootError::NoEntry(id) => write!(f, "the ISO has no boot entry \"{id}\""),
            BootError::NoKernel => f.write_str("its boot entry names no kernel"),
            BootError::NoInitrd => f.write_str("its boot entry names no initramfs"),
            BootError::UnknownVariable(v) => {
                write!(f, "its boot entry uses ${{{v}}}, which Borea doesn't know")
            }
            BootError::Unsafe(what) => write!(f, "its boot entry has an unsafe value: {what}"),
            BootError::NotLoopback => f.write_str("its boot entry does not boot from an ISO file"),
        }
    }
}

/// GRUB-style words: whitespace separated, double quotes grouping.
fn words(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut word = String::new();
    let mut quoted = false;
    let mut any = false;
    for c in line.chars() {
        match c {
            '"' => {
                quoted = !quoted;
                any = true;
            }
            c if c.is_whitespace() && !quoted => {
                if any {
                    out.push(std::mem::take(&mut word));
                    any = false;
                }
            }
            c => {
                word.push(c);
                any = true;
            }
        }
    }
    if any {
        out.push(word);
    }
    out
}

/// The first unconditional `set name=value` in the file.
fn top_level_set(config: &str, name: &str) -> Option<String> {
    let mut depth = 0i32;
    for line in config.lines().map(str::trim) {
        if line.starts_with("if ") {
            depth += 1;
        } else if line == "fi" {
            depth -= 1;
        } else if depth == 0 {
            if let Some(value) = line.strip_prefix(&format!("set {name}=")) {
                return Some(value.trim_matches(['"', '\'']).to_owned());
            }
        }
    }
    None
}

fn expand(word: &str, vars: &[(&str, &str)]) -> Result<String, BootError> {
    let mut out = String::new();
    let mut rest = word;
    while let Some(at) = rest.find("${") {
        out.push_str(&rest[..at]);
        let after = &rest[at + 2..];
        let end = after.find('}').ok_or_else(|| BootError::Unsafe(word.into()))?;
        let name = &after[..end];
        let value = vars
            .iter()
            .find(|(n, _)| *n == name)
            .map(|(_, v)| *v)
            .ok_or_else(|| BootError::UnknownVariable(name.into()))?;
        out.push_str(value);
        rest = &after[end + 1..];
    }
    out.push_str(rest);
    Ok(out)
}

fn iso_path(path: &str) -> Result<String, BootError> {
    if !path.starts_with('/') || path.split('/').any(|p| p == "..") || path.contains('\0') {
        return Err(BootError::Unsafe(path.into()));
    }
    Ok(path.to_owned())
}

/// The installer's own boot entry `entry` in `config` (a loopback.cfg), for the
/// ISO at `iso`.
pub fn boot_spec(config: &str, entry: &str, iso: &IsoFile<'_>) -> Result<BootSpec, BootError> {
    for value in [iso.uuid, iso.path] {
        if value.is_empty() || value.chars().any(|c| c.is_whitespace() || c == '"' || c == '$') {
            return Err(BootError::Unsafe(value.into()));
        }
    }
    let kernel_var = top_level_set(config, "live_kernel");
    let mut vars: Vec<(&str, &str)> =
        vec![("archiso_img_dev_uuid", iso.uuid), ("iso_path", iso.path)];
    if let Some(kernel) = &kernel_var {
        vars.push(("live_kernel", kernel));
    }

    let marker = format!("--id '{entry}'");
    let lines: Vec<&str> = config.lines().collect();
    let start = lines
        .iter()
        .position(|l| {
            let l = l.trim();
            l.starts_with("menuentry ")
                && (l.contains(&marker) || l.contains(&format!("--id \"{entry}\"")))
        })
        .ok_or_else(|| BootError::NoEntry(entry.into()))?;
    let mut kernel = None;
    let mut cmdline = Vec::new();
    let mut initrds = Vec::new();
    for line in lines[start + 1..].iter().map(|l| l.trim()) {
        if line == "}" {
            break;
        }
        let w = words(line);
        match w.first().map(String::as_str) {
            Some("linux") => {
                let mut rest = w.iter().skip(1);
                kernel = Some(iso_path(&expand(rest.next().ok_or(BootError::NoKernel)?, &vars)?)?);
                for arg in rest {
                    cmdline.push(expand(arg, &vars)?);
                }
            }
            Some("initrd") => {
                for file in w.iter().skip(1) {
                    initrds.push(iso_path(&expand(file, &vars)?)?);
                }
            }
            _ => {}
        }
    }
    let kernel = kernel.ok_or(BootError::NoKernel)?;
    if initrds.is_empty() {
        return Err(BootError::NoInitrd);
    }
    let has =
        |key: &str, want: &str| cmdline.iter().any(|a| a.split_once('=') == Some((key, want)));
    if !has("img_dev", &format!("UUID={}", iso.uuid)) || !has("img_loop", iso.path) {
        return Err(BootError::NotLoopback);
    }
    Ok(BootSpec { kernel, initrds, cmdline: cmdline.join(" ") })
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic)]
mod tests {
    use super::*;

    /// omacom-io/omarchy-iso configs/grub/loopback.cfg at 4df07ef, with the
    /// substitutions builder/build-iso.sh makes for an x86-64 release.
    fn omarchy() -> String {
        include_str!("../tests/omarchy-loopback.cfg")
            .replace("%INSTALL_DIR%", "arch")
            .replace("%ARCH%", "x86_64")
            .replace("%KERNEL%", "linux-omarchy")
            .replace("%BOOT_SPLASH_KERNEL_OPTIONS%", "quiet splash ")
            .replace("%KERNEL_OPTIONS%", "xe.enable_panel_replay=0")
            .replace("%N1X_ONLY_BEGIN%", "")
            .replace("%N1X_ONLY_END%", "")
    }

    const ISO: IsoFile<'static> =
        IsoFile { uuid: "0f1e2d3c-4b5a-6978-8796-a5b4c3d2e1f0", path: "/borea/omarchy-4.0.6.iso" };

    #[test]
    fn boots_omarchys_installer_from_the_iso_file() {
        let spec = boot_spec(&omarchy(), "archlinux", &ISO).unwrap();
        assert_eq!(spec.kernel, "/arch/boot/x86_64/vmlinuz-linux-omarchy");
        assert_eq!(spec.initrds, ["/arch/boot/x86_64/initramfs-linux-omarchy.img"]);
        assert_eq!(
            spec.cmdline,
            "archisobasedir=arch img_dev=UUID=0f1e2d3c-4b5a-6978-8796-a5b4c3d2e1f0 \
             img_loop=/borea/omarchy-4.0.6.iso copytoram=n initramfs_options=size=90% \
             quiet splash xe.enable_panel_replay=0"
        );
        // The Mac-only kernel switch inside `if` is not taken.
        assert!(!spec.kernel.contains("t2"));
    }

    /// The loopback.cfg inside the pinned Omarchy 4.0.6 ISO: one live kernel
    /// (linux-t2) for every machine. The VM test hands off with exactly this.
    #[test]
    fn boots_the_pinned_4_0_6_release() {
        let config = include_str!("../tests/omarchy-4.0.6-loopback.cfg")
            .replace("%INSTALL_DIR%", "arch")
            .replace("%ARCH%", "x86_64");
        let spec = boot_spec(&config, "archlinux", &ISO).unwrap();
        assert_eq!(spec.kernel, "/arch/boot/x86_64/vmlinuz-linux-t2");
        assert_eq!(spec.initrds, ["/arch/boot/x86_64/initramfs-linux-t2.img"]);
        assert_eq!(
            spec.cmdline,
            "archisobasedir=arch img_dev=UUID=0f1e2d3c-4b5a-6978-8796-a5b4c3d2e1f0 \
             img_loop=/borea/omarchy-4.0.6.iso quiet splash xe.enable_panel_replay=0 initramfs_async=0"
        );
    }

    #[test]
    fn refuses_entries_it_cannot_trust() {
        let config = omarchy();
        assert_eq!(boot_spec(&config, "nope", &ISO), Err(BootError::NoEntry("nope".into())));
        let odd =
            config.replace("img_loop=\"${iso_path}\"", "img_loop=\"${iso_path}\" x=${secret}");
        assert_eq!(
            boot_spec(&odd, "archlinux", &ISO),
            Err(BootError::UnknownVariable("secret".into()))
        );
        let escape = config.replace("/arch/boot/x86_64/vmlinuz", "/arch/../../vmlinuz");
        assert!(matches!(boot_spec(&escape, "archlinux", &ISO), Err(BootError::Unsafe(_))));
        let plain =
            config.replace(" img_dev=UUID=${archiso_img_dev_uuid} img_loop=\"${iso_path}\"", "");
        assert_eq!(boot_spec(&plain, "archlinux", &ISO), Err(BootError::NotLoopback));
        let spaced = IsoFile { uuid: ISO.uuid, path: "/my iso.iso" };
        assert!(matches!(boot_spec(&config, "archlinux", &spaced), Err(BootError::Unsafe(_))));
    }

    #[test]
    fn splits_words_like_grub() {
        assert_eq!(words(r#"linux /k a="b c" d"#), ["linux", "/k", "a=b c", "d"]);
        assert_eq!(words(r#"img_loop="${iso_path}""#), ["img_loop=${iso_path}"]);
    }
}
