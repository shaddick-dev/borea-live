//! `borea plan`: resolve and verify an installation, write the plan for review,
//! and say exactly how to approve it. Nothing here changes disks.

use std::io::Write;
use std::path::PathBuf;

use borea_catalog::Catalog;
use borea_engine::{InstallRequest, arch};

use crate::Result;
use crate::ui::{Out, section};

pub const USAGE: &str = "\
borea plan --distro DISTRO --channel CHANNEL --edition EDITION
           --disk NAME --user NAME --hostname NAME --timezone Area/City
           --locale LOCALE --keymap KEYMAP (--extras none | --extra ID ...)
           [--kernel-arg ARG]... [-o PLAN.json]";

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
        distro: required("--distro")?,
        edition: required("--edition")?,
        disk: required("--disk")?,
        hostname: required("--hostname")?,
        username: required("--user")?,
        timezone: required("--timezone")?,
        locale: required("--locale")?,
        keymap: required("--keymap")?,
        extras: all("--extra"),
        kernel_args: all("--kernel-arg"),
    };
    let target = borea_engine::online::Target::parse(&request.distro)?;
    let channel = required("--channel")?;
    if channel != target.profile().channel {
        return Err(format!(
            "channel `{channel}` is not offered for {}; choose explicitly from `borea targets`",
            request.distro
        )
        .into());
    }
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
            "--channel",
            "--extras",
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
    for flag in [
        "--distro",
        "--channel",
        "--edition",
        "--disk",
        "--user",
        "--hostname",
        "--timezone",
        "--locale",
        "--keymap",
    ] {
        if !singles.contains(flag) {
            return Err(format!("{flag} must be chosen explicitly\n{USAGE}"));
        }
    }
    let extras = args.windows(2).find(|w| w[0] == "--extras").map(|w| w[1]);
    let has_extra = args.contains(&"--extra");
    if !matches!((extras, has_extra), (Some("none"), false) | (None, true)) {
        return Err("choose --extras none or one or more --extra ID options".into());
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

    fn chosen<'a>(distro: &'a str, channel: &'a str) -> Vec<&'a str> {
        vec![
            "--distro",
            distro,
            "--channel",
            channel,
            "--edition",
            "minimal",
            "--disk",
            "vda",
            "--user",
            "luke",
            "--hostname",
            "borea",
            "--timezone",
            "Europe/London",
            "--locale",
            "en_GB.UTF-8",
            "--keymap",
            "uk",
            "--extras",
            "none",
        ]
    }

    #[test]
    fn every_install_setting_must_be_picked() {
        let complete = chosen("arch", "rolling");
        for pair in 0..complete.len() / 2 {
            let mut incomplete = complete.clone();
            incomplete.drain(pair * 2..pair * 2 + 2);
            assert!(
                validate_args(&incomplete).is_err(),
                "missing {} was accepted",
                complete[pair * 2]
            );
        }
        let mut conflict = complete.clone();
        conflict.extend(["--extra", "printing"]);
        assert!(validate_args(&conflict).is_err());
        let mut picked = complete;
        picked.truncate(picked.len() - 2);
        picked.extend(["--extra", "printing", "--extra", "codecs"]);
        validate_args(&picked).unwrap();
    }

    #[test]
    fn never_silently_ignores_a_target_or_misspelled_option() {
        assert!(validate_args(&["--dist", "fedora"]).is_err());
        assert!(validate_args(&["--distro"]).is_err());
        assert!(validate_args(&["--distro", "arch", "--distro", "fedora"]).is_err());
        validate_args(&chosen("arch", "rolling")).unwrap();
    }

    #[test]
    fn unavailable_adapter_is_rejected_before_hardware_or_downloads() {
        for distro in ["fedora", "debian", "opensuse"] {
            let error = run(
                &mut std::io::stdout().lock(),
                &chosen(
                    distro,
                    borea_engine::online::Target::parse(distro).unwrap().profile().channel,
                ),
            )
            .unwrap_err()
            .to_string();
            assert!(error.contains("execution adapter is not implemented"), "{error}");
        }
    }
}
