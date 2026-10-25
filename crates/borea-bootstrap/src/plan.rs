//! `borea plan`: resolve and verify an installation, write the plan for review,
//! and say exactly how to approve it. Nothing here changes disks.

use std::io::Write;
use std::path::PathBuf;

use borea_catalog::Catalog;
use borea_engine::{InstallRequest, arch};

use crate::Result;
use crate::ui::{Out, section};

pub const USAGE: &str = "\
borea plan [--distro arch] --disk NAME --user NAME [--edition minimal] [--hostname NAME]
           [--timezone Area/City] [--locale en_US.UTF-8] [--keymap us]
           [--extra ID]... [--kernel-arg ARG]... [-o PLAN.json]";

pub fn run(out: &mut Out, args: &[&str]) -> Result {
    validate_args(args)?;
    let value = |flag: &str| {
        args.iter().position(|a| *a == flag).and_then(|i| args.get(i + 1)).map(|v| v.to_string())
    };
    let all = |flag: &str| -> Vec<String> {
        args.windows(2).filter(|w| w[0] == flag).map(|w| w[1].to_string()).collect()
    };
    let required = |flag: &str| value(flag).ok_or_else(|| format!("{flag} is required\n\n{USAGE}"));
    let request = InstallRequest {
        distro: value("--distro").unwrap_or_else(|| "arch".into()),
        edition: value("--edition").unwrap_or_else(|| "minimal".into()),
        disk: required("--disk")?,
        hostname: value("--hostname").unwrap_or_else(|| "archlinux".into()),
        username: required("--user")?,
        timezone: value("--timezone").unwrap_or_else(|| "UTC".into()),
        locale: value("--locale").unwrap_or_else(|| "en_US.UTF-8".into()),
        keymap: value("--keymap").unwrap_or_else(|| "us".into()),
        extras: all("--extra"),
        kernel_args: all("--kernel-arg"),
    };
    let target = borea_engine::online::Target::parse(&request.distro)?;
    if target != borea_engine::online::Target::Arch {
        return Err(format!(
            "{} / {}: execution adapter is not implemented yet; no plan was written and no disk was changed",
            request.distro, target.profile().channel
        ).into());
    }
    let output = PathBuf::from(value("-o").unwrap_or_else(|| "borea-plan.json".into()));

    let catalog = Catalog::bundled()?;
    let hw = borea_hardware::scan();
    let root = PathBuf::from("/");
    arch::check(&request, &catalog, &hw, &root)?;

    let cache = cache_dir();
    writeln!(out, "Getting Arch Linux's bootstrap archive and checking its signature…")?;
    out.flush()?;
    let (bootstrap, archive) = arch::resolve_bootstrap(&cache, |_| {})?;
    let plan = arch::plan(&request, &catalog, &hw, &root, bootstrap)?;
    std::fs::write(&output, plan.to_json())?;

    section(out, "Installation plan")?;
    for line in plan.review() {
        writeln!(out, "  · {line}")?;
    }
    let digest = plan.digest();
    writeln!(out, "\n  Plan      {}", output.display())?;
    writeln!(out, "  Digest    {digest}")?;
    writeln!(out, "\n  Nothing has been written. To install, erasing {}:", plan.disk.name)?;
    writeln!(
        out,
        "    sudo borea-worker {} --approve {digest} --bootstrap {}",
        output.display(),
        archive.display()
    )?;
    Ok(())
}

fn validate_args(args: &[&str]) -> std::result::Result<(), String> {
    let mut singles = std::collections::HashSet::new();
    let mut pairs = args.chunks_exact(2);
    for pair in &mut pairs {
        let (flag, value) = (pair[0], pair[1]);
        if ![
            "--distro",
            "--disk",
            "--user",
            "--edition",
            "--hostname",
            "--timezone",
            "--locale",
            "--keymap",
            "--extra",
            "--kernel-arg",
            "-o",
        ]
        .contains(&flag)
        {
            return Err(format!("unknown plan option `{flag}`\n{USAGE}"));
        }
        if value.is_empty() || value.starts_with('-') {
            return Err(format!("{flag} needs a value\n{USAGE}"));
        }
        if !["--extra", "--kernel-arg"].contains(&flag) && !singles.insert(flag) {
            return Err(format!("{flag} was specified more than once"));
        }
    }
    if let Some(flag) = pairs.remainder().first() {
        return Err(format!("{flag} needs a value\n{USAGE}"));
    }
    Ok(())
}

/// Where downloads are kept between planning and installing.
fn cache_dir() -> PathBuf {
    let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| "/tmp".into());
    std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".cache"))
        .join("borea")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn never_silently_ignores_a_target_or_misspelled_option() {
        assert!(validate_args(&["--dist", "fedora"]).is_err());
        assert!(validate_args(&["--distro"]).is_err());
        assert!(validate_args(&["--distro", "arch", "--distro", "fedora"]).is_err());
        validate_args(&["--distro", "arch", "--extra", "printing", "--extra", "codecs"]).unwrap();
    }

    #[test]
    fn unavailable_adapter_is_rejected_before_hardware_or_downloads() {
        for distro in ["fedora", "debian", "opensuse"] {
            let error = run(
                &mut std::io::stdout().lock(),
                &["--distro", distro, "--disk", "vda", "--user", "luke"],
            )
            .unwrap_err()
            .to_string();
            assert!(error.contains("execution adapter is not implemented"), "{error}");
        }
    }
}
