#![forbid(unsafe_code)]
//! Never a production installer entry point. Requires a QEMU firmware marker and a named disposable disk.
use borea_core::*;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};
mod fixture;
mod repair;
fn run() -> Result<(), Hint> {
    let marker = "/sys/firmware/qemu_fw_cfg/by_name/opt/borea/autotest/raw";
    if !Path::new(marker).is_file()
        || fs::read_to_string(marker).map_err(|_| hint("vm", "No test marker."))?.trim()
            != "BOREA_DISPOSABLE_VM"
    {
        return Err(hint(
            "vm",
            "This test binary only runs in an explicitly marked disposable QEMU guest.",
        ));
    }
    let mut facts = borea_disk::probe()?;
    println!("BOREA_DISK_INVENTORY {facts:?}");
    let disk = facts
        .disks
        .iter()
        .find(|d| d.id.serial.as_deref() == Some("BOREA-TEST-TARGET"))
        .ok_or_else(|| hint("vm", "The disposable target disk was not found."))?
        .clone();
    if fs::read_to_string("/sys/firmware/qemu_fw_cfg/by_name/opt/borea/repair/raw").ok().as_deref()
        == Some("debian-boot")
    {
        return repair::debian_boot(&disk);
    }
    let alongside = fs::read_to_string("/sys/firmware/qemu_fw_cfg/by_name/opt/borea/layout/raw")
        .ok()
        .as_deref()
        == Some("alongside");
    if alongside {
        fixture::prepare(&disk)?;
        facts = borea_disk::probe()?;
    }
    let disk = facts
        .disks
        .iter()
        .find(|d| d.id == disk.id)
        .ok_or_else(|| hint("vm", "The test disk changed during fixture preparation."))?;
    let (preserved, disk_plan) = if alongside {
        let (fixture, plan) = fixture::Preserved::select(disk)?;
        (Some(fixture), plan)
    } else {
        (None, DiskPlan::Whole { disk: disk.id.clone() })
    };
    let distro = match fs::read_to_string("/sys/firmware/qemu_fw_cfg/by_name/opt/borea/distro/raw")
        .unwrap_or_else(|_| "debian".into())
        .as_str()
    {
        "arch" => Distribution::Arch,
        "debian" => Distribution::Debian,
        "fedora" => Distribution::Fedora,
        "opensuse" => Distribution::OpenSuse,
        _ => return Err(hint("vm", "Unknown test distribution.")),
    };
    let noctalia = fs::read_to_string("/sys/firmware/qemu_fw_cfg/by_name/opt/borea/desktop/raw")
        .ok()
        .as_deref()
        == Some("noctalia");
    if noctalia && distro != Distribution::Arch {
        return Err(hint("vm", "The Noctalia scenario requires Arch."));
    }
    let plan = Plan {
        extras: BTreeSet::new(),
        version: 1,
        net: NetChoice::Ethernet,
        locale: LocaleChoice {
            lang: "en_GB.UTF-8".into(),
            keyboard: Keyboard {
                layout: "gb".into(),
                variant: String::new(),
                console: "uk".into(),
            },
            timezone: "Europe/London".into(),
        },
        distro,
        secure_boot: None,
        release: match distro {
            Distribution::Arch => "rolling",
            Distribution::Debian => "trixie",
            Distribution::Fedora => "44",
            Distribution::OpenSuse => "tumbleweed",
        }
        .into(),
        boot_arguments: vec!["console=ttyS0,115200".into()],
        desktop: noctalia.then_some(borea_core::Desktop::HyprlandNoctalia),
        env: if noctalia { "composed" } else { "minimal" }.into(),
        addons: BTreeSet::new(),
        picks: Vec::new(),
        apps: BTreeSet::new(),
        prefer: SourcePreference::Native,
        sources: BTreeMap::new(),
        disk: disk_plan,
        user: Identity {
            full_name: "Disposable VM Test".into(),
            username: "boreatest".into(),
            hostname: "borea-test-vm".into(),
            same_passphrase: true,
        },
    };
    let hw = borea_catalog::probe::probe().map_err(|e| hint("vm", e.to_string()))?;
    println!("BOREA_HARDWARE_PROBED");
    if fs::read_to_string("/sys/firmware/qemu_fw_cfg/by_name/opt/borea/secure-boot/raw")
        .ok()
        .as_deref()
        == Some("1")
        && !hw.secure_boot
    {
        return Err(hint(
            "vm",
            "The Secure Boot test must start with firmware enforcement enabled.",
        ));
    }
    borea_net::check_mirror(borea_backend::mirror(distro))?;
    println!("BOREA_MIRROR_REACHABLE");
    let bootstrap = borea_backend::bootstrap::resolve(&plan)?;
    println!("BOREA_BOOTSTRAP_RESOLVED");
    let mut job = borea_job::compile(&plan, &hw, &facts, &bootstrap)?;
    borea_job::resolve_external(&mut job)?;
    let approved = job.digest()?;
    println!("BOREA_JOB_COMPILED");
    // Public test fixture only. Never used as a production default or written to a plan.
    let secrets = Secrets {
        wifi: None,
        passphrase: Secret::new("borea-disposable-vm-ONLY".into()),
        admin: None,
    };
    if fs::read_to_string("/sys/firmware/qemu_fw_cfg/by_name/opt/borea/preflight/raw")
        .ok()
        .as_deref()
        == Some("1")
    {
        let stat = format!(
            "/sys/class/block/{}/stat",
            disk.id
                .path
                .file_name()
                .and_then(|s| s.to_str())
                .ok_or_else(|| hint("vm", "No test disk name."))?
        );
        let writes = || -> Result<Vec<u64>, Hint> {
            let text = fs::read_to_string(&stat).map_err(|e| hint("vm", e.to_string()))?;
            let fields = text
                .split_whitespace()
                .map(|s| s.parse::<u64>())
                .collect::<Result<Vec<_>, _>>()
                .map_err(|e| hint("vm", e.to_string()))?;
            Ok(vec![
                *fields.get(4).ok_or_else(|| hint("vm", "No disk write counter."))?,
                *fields.get(6).ok_or_else(|| hint("vm", "No disk sector counter."))?,
            ])
        };
        let before = writes()?;
        borea_exec::verify_sources(job, secrets, &approved, &facts, &mut |event| {
            if let borea_exec::Event::Line { text, .. } = event {
                println!("{text}");
            }
        })?;
        if before != writes()? {
            return Err(hint("vm", "Preparation wrote to the target disk."));
        }
        println!("BOREA_PREFLIGHT_COMPLETE");
        return Ok(());
    }
    let mut session = borea_exec::Session::start(job, secrets, &approved, &facts)?;
    session.run(&mut |event| match event {
        borea_exec::Event::StageStarted { title, .. } => println!("BOREA_STAGE {title}"),
        borea_exec::Event::Line { text, .. } => println!("{text}"),
        borea_exec::Event::Failed { fault } => {
            println!("BOREA_FAILED {}", fault.title);
            println!("BOREA_COMMAND {}", fault.command);
            for line in fault.stderr_tail {
                println!("BOREA_COMMAND_STDERR {line}");
            }
        }
        borea_exec::Event::Finished => println!("BOREA_INSTALL_COMPLETE"),
        _ => {}
    })?;
    if let Some(preserved) = preserved {
        preserved.verify(&borea_disk::probe()?)?;
    }
    Ok(())
}
fn main() -> std::process::ExitCode {
    match run() {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("BOREA_TEST_ERROR {e}");
            std::process::ExitCode::FAILURE
        }
    }
}
