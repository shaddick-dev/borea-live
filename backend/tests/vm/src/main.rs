#![forbid(unsafe_code)]
//! Never a production installer entry point. Requires a QEMU firmware marker and a named disposable disk.
use borea_core::*;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};
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
    let facts = borea_disk::probe()?;
    println!("BOREA_DISK_INVENTORY {facts:?}");
    let disk = facts
        .disks
        .iter()
        .find(|d| d.id.serial.as_deref() == Some("BOREA-TEST-TARGET"))
        .ok_or_else(|| hint("vm", "The disposable target disk was not found."))?;
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
    let plan = Plan {
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
        desktop: None,
        env: "minimal".into(),
        addons: BTreeSet::new(),
        picks: Vec::new(),
        apps: BTreeSet::new(),
        prefer: SourcePreference::Native,
        sources: BTreeMap::new(),
        disk: DiskPlan::Whole { disk: disk.id.clone() },
        user: Identity {
            full_name: "Disposable VM Test".into(),
            username: "boreatest".into(),
            hostname: "borea-test-vm".into(),
            same_passphrase: true,
        },
    };
    let hw = borea_catalog::probe::probe().map_err(|e| hint("vm", e.to_string()))?;
    borea_net::check_mirror(borea_backend::mirror(distro))?;
    let bootstrap = borea_backend::bootstrap::resolve(&plan)?;
    let mut job = borea_job::compile(&plan, &hw, &facts, &bootstrap)?;
    borea_job::resolve_external(&mut job)?;
    let approved = job.digest()?;
    // Public test fixture only. Never used as a production default or written to a plan.
    let secrets = Secrets {
        wifi: None,
        passphrase: Secret::new("borea-disposable-vm-ONLY".into()),
        admin: None,
    };
    let mut session = borea_exec::Session::start(job, secrets, &approved, &facts)?;
    session.run(&mut |event| match event {
        borea_exec::Event::StageStarted { title, .. } => println!("BOREA_STAGE {title}"),
        borea_exec::Event::Line { text, .. } => println!("{text}"),
        borea_exec::Event::Failed { fault } => println!("BOREA_FAILED {}", fault.title),
        borea_exec::Event::Finished => println!("BOREA_INSTALL_COMPLETE"),
        _ => {}
    })
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
