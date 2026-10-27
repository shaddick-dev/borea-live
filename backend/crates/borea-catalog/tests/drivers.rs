#![forbid(unsafe_code)]
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]
use std::fs;
use std::path::Path;

use borea_catalog::hardware::{pci_ids, samples};
use borea_catalog::*;

fn sample(id: &str) -> Hardware {
    samples().into_iter().find(|(k, _)| *k == id).unwrap().1
}

fn names(p: &DriverPlan) -> Vec<&'static str> {
    p.packages.iter().map(|x| x.name).collect()
}

fn from(p: &DriverPlan, name: &str) -> Source {
    p.packages
        .iter()
        .find(|x| x.name == name)
        .unwrap_or_else(|| panic!("{name} not in {:?}", names(p)))
        .source
}

#[test]
fn nvidia_generations_from_device_ids() {
    for (id, generation) in [
        (0x2520, NvGen::Ampere),    // RTX 3060 Laptop (TUF F15)
        (0x2f04, NvGen::Blackwell), // RTX 5070
        (0x2684, NvGen::Ada),       // RTX 4090
        (0x28a0, NvGen::Ada),       // RTX 4060 Laptop
        (0x1f82, NvGen::Turing),    // GTX 1650
        (0x2182, NvGen::Turing),    // GTX 1660 Ti
        (0x1e04, NvGen::Turing),    // RTX 2080 Ti
        (0x1b80, NvGen::Pascal),    // GTX 1080
        (0x1c82, NvGen::Pascal),    // GTX 1050 Ti
        (0x1401, NvGen::Maxwell),   // GTX 960
        (0x1380, NvGen::Maxwell),   // GTX 750 Ti
        (0x1180, NvGen::Kepler),    // GTX 680
        (0x0de1, NvGen::Older),     // GT 430
    ] {
        assert_eq!(NvGen::from_device(id), generation, "{id:04x}");
    }
}

#[test]
fn right_nvidia_driver_per_generation_and_distro() {
    use Distro::*;
    for d in Distro::ALL {
        assert_eq!(nvidia_driver(NvGen::Ampere, d), NvDriver::Open);
        assert_eq!(nvidia_driver(NvGen::Pascal, d), NvDriver::Legacy);
        assert_eq!(nvidia_driver(NvGen::Kepler, d), NvDriver::Nouveau);
    }
    assert_eq!(nvidia_driver(NvGen::Blackwell, Arch), NvDriver::Open);
    assert_eq!(nvidia_driver(NvGen::Blackwell, Debian), NvDriver::Nouveau, "Debian 13 ships 550");
}

#[test]
fn tuf_f15_hybrid_laptop_on_every_distro() {
    let hw = sample("tuf-f15");
    assert!(hw.is_laptop() && hw.hybrid());
    assert_eq!(hw.display_name(), "ASUS TUF Gaming F15");
    assert_eq!(hw.mux.as_ref().unwrap().mode, MuxMode::Hybrid);

    let a = plan_drivers(&hw, Distro::Arch);
    assert_eq!(from(&a, "nvidia-open"), Source::Repo("arch.core-extra"));
    assert_eq!(from(&a, "lib32-nvidia-utils"), Source::Repo("arch.multilib"));
    for n in [
        "intel-media-driver",
        "vulkan-intel",
        "nvidia-prime",
        "switcheroo-control",
        "intel-ucode",
        "sof-firmware",
        "power-profiles-daemon",
    ] {
        assert!(names(&a).contains(&n), "arch missing {n}");
    }
    assert!(
        a.services.contains(&"nvidia-powerd.service")
            && a.services.contains(&"nvidia-suspend.service")
    );
    assert!(a.found.iter().any(|f| f.what == "MUX switch"));

    let f = plan_drivers(&hw, Distro::Fedora);
    assert_eq!(from(&f, "akmod-nvidia"), Source::Repo("fedora.rpmfusion-nonfree"));
    assert!(
        f.sources.contains(&Source::Repo("fedora.rpmfusion-nonfree")),
        "nonfree is off by default, the drivers switch it on"
    );
    assert!(f.mok, "akmods are self-signed and Secure Boot is on");

    let o = plan_drivers(&hw, Distro::OpenSuse);
    assert_eq!(from(&o, "nvidia-open-driver-G07-signed-kmp-meta"), Source::Repo("opensuse.oss"));
    assert_eq!(from(&o, "nvidia-gl-G07"), Source::Repo("opensuse.nvidia"));
    assert!(!o.mok, "openSUSE signs the open modules");

    let d = plan_drivers(&hw, Distro::Debian);
    assert_eq!(from(&d, "nvidia-open-kernel-dkms"), Source::Repo("debian.contrib"));
    assert_eq!(from(&d, "intel-microcode"), Source::Repo("debian.non-free-firmware"));
    assert!(d.kernel_args.contains(&"nvidia-drm.modeset=1") && d.mok);
}

#[test]
fn pascal_desktop_gets_the_580_legacy_branch() {
    let hw = sample("gtx1080");
    assert!(!hw.is_laptop() && !hw.hybrid());
    let a = plan_drivers(&hw, Distro::Arch);
    assert_eq!(from(&a, "nvidia-580xx-dkms"), Source::Aur);
    assert!(!names(&a).contains(&"nvidia-open"));
    assert!(a.services.is_empty(), "no laptop power services on a desktop");
    assert_eq!(
        from(&plan_drivers(&hw, Distro::Fedora), "akmod-nvidia-580xx"),
        Source::Repo("fedora.rpmfusion-nonfree")
    );
    assert!(names(&plan_drivers(&hw, Distro::Debian)).contains(&"nvidia-kernel-dkms"));
    assert!(names(&plan_drivers(&hw, Distro::Arch)).contains(&"amd-ucode"));
}

#[test]
fn rtx50_on_debian_warns_and_falls_back() {
    let hw = sample("rtx5070");
    let d = plan_drivers(&hw, Distro::Debian);
    assert!(d.warning.as_deref().unwrap().contains("too old"));
    assert!(!names(&d).contains(&"nvidia-driver"));
    let f = plan_drivers(&hw, Distro::Fedora);
    assert!(f.warning.is_none() && names(&f).contains(&"akmod-nvidia"));
    assert!(plan_drivers(&hw, Distro::Arch).warning.unwrap().contains("Secure Boot"));
}

#[test]
fn amd_and_intel_need_no_proprietary_driver() {
    let amd = plan_drivers(&sample("rx7800"), Distro::Fedora);
    assert_eq!(from(&amd, "mesa-va-drivers-freeworld"), Source::Repo("fedora.rpmfusion-free"));
    assert!(!amd.sources.contains(&Source::Repo("fedora.rpmfusion-nonfree")));
    assert!(
        names(&plan_drivers(&sample("rx7800"), Distro::Debian)).contains(&"firmware-amd-graphics")
    );

    let tp = sample("thinkpad");
    assert_eq!(tp.display_name(), "Lenovo ThinkPad X1 Carbon Gen 11");
    let a = plan_drivers(&tp, Distro::Arch);
    assert!(!a.packages.iter().any(|p| p.name.contains("nvidia")));
    assert!(names(&a).contains(&"fprintd") && !names(&a).contains(&"switcheroo-control"));
}

#[test]
fn driver_sources_are_locked_on() {
    for (_, hw) in samples() {
        for d in Distro::ALL {
            let p = plan_drivers(&hw, d);
            let mut s = Sources::defaults(d);
            s.require(&p.sources).unwrap_or_else(|e| panic!("{d:?}: {e:?}"));
            for src in &p.sources {
                assert!(s.is_on(*src));
                assert!(s.switches().contains(src), "{src:?} must be visible on {d:?}");
                assert!(
                    matches!(s.set(*src, false), Err(SourceError::NeededByDrivers(_)))
                        || matches!(src, Source::Repo(id) if repo_is_base(id))
                );
            }
        }
    }
}

fn repo_is_base(id: &str) -> bool {
    REPOS.iter().any(|r| r.id == id && r.base)
}

#[test]
fn scans_a_real_sysfs_layout() {
    let root = std::env::temp_dir().join(format!("borea-hw-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    let w = |p: &str, v: &[u8]| {
        let f = root.join(p);
        fs::create_dir_all(f.parent().unwrap()).unwrap();
        fs::write(f, v).unwrap();
    };
    for (k, v) in [
        ("sys_vendor", "ASUSTeK COMPUTER INC."),
        ("product_name", "ASUS TUF Gaming F15 FX507ZM_FX507ZM"),
        ("board_name", "FX507ZM"),
        ("chassis_type", "10"),
    ] {
        w(&format!("sys/class/dmi/id/{k}"), format!("{v}\n").as_bytes());
    }
    w("sys/class/power_supply/BAT1/type", b"Battery\n");
    w("proc/cpuinfo", b"vendor_id\t: GenuineIntel\n");
    let pci = |slot: &str, class: &str, v: &str, d: &str, boot: Option<&str>| {
        w(&format!("sys/bus/pci/devices/{slot}/class"), class.as_bytes());
        w(&format!("sys/bus/pci/devices/{slot}/vendor"), v.as_bytes());
        w(&format!("sys/bus/pci/devices/{slot}/device"), d.as_bytes());
        if let Some(b) = boot {
            w(&format!("sys/bus/pci/devices/{slot}/boot_vga"), b.as_bytes());
        }
    };
    pci("0000:00:02.0", "0x030000", "0x8086", "0x46a6", Some("1"));
    pci("0000:01:00.0", "0x030000", "0x10de", "0x2520", Some("0"));
    pci("0000:00:1f.3", "0x040100", "0x8086", "0x51c8", None);
    w("sys/class/drm/card1-eDP-1/status", b"connected\n");
    w("sys/class/drm/card1/device/slot", b"0000:00:02.0\n");
    w("sys/class/firmware-attributes/asus-armoury/attributes/gpu_mux_mode/current_value", b"1\n");
    w("sys/firmware/efi/efivars/SecureBoot-8be4df61-93ca-11d2-aa0d-00e098032b8c", &[6, 0, 0, 0, 1]);
    w("sys/bus/usb/devices/3-6/idVendor", b"27c6\n");

    let ids = "10de  NVIDIA Corporation\n\t2520  GA106M [GeForce RTX 3060 Mobile / Max-Q]\n8086  Intel Corporation\n\t46a6  Alder Lake-P GT2 [Iris Xe Graphics]\n";
    let hw = scan(Path::new(&root), &pci_ids(ids)).unwrap();
    let _ = fs::remove_dir_all(&root);

    assert_eq!(hw.display_name(), "ASUS TUF Gaming F15");
    assert!(hw.is_laptop() && hw.hybrid() && hw.secure_boot && hw.sof_audio);
    assert_eq!(hw.cpu, Cpu::Intel);
    assert_eq!(hw.nvidia(), Some(NvGen::Ampere));
    assert_eq!(hw.discrete().unwrap().name, "GeForce RTX 3060 Mobile");
    assert_eq!(hw.integrated().unwrap().name, "Intel Iris Xe Graphics");
    assert!(hw.integrated().unwrap().drives_panel);
    assert_eq!(hw.mux.as_ref().map(|m| m.mode), Some(MuxMode::Hybrid));
    assert_eq!(hw.fingerprint.as_deref(), Some("Goodix"));
}

#[test]
fn pci_names_become_names_people_know() {
    use borea_catalog::hardware::tidy_name;
    assert_eq!(
        tidy_name(0x10de, "AD107M [GeForce RTX 4060 Max-Q / Mobile]"),
        "GeForce RTX 4060 Max-Q"
    );
    assert_eq!(
        tidy_name(0x1002, "Navi 32 [Radeon RX 7700 XT / 7800 XT]"),
        "Radeon RX 7700 XT / 7800 XT"
    );
    assert_eq!(tidy_name(0x8086, "Alder Lake-P GT2 [Iris Xe Graphics]"), "Intel Iris Xe Graphics");
    assert_eq!(tidy_name(0x10de, "TU117M"), "TU117M");
}

#[test]
fn probe_report_is_json_the_installer_can_read() {
    let hw = sample("tuf-f15");
    let j = borea_catalog::probe::report(&hw);
    assert!(j.starts_with('{') && j.ends_with('}'));
    for k in [
        "\"name\":\"ASUS TUF Gaming F15\"",
        "\"laptop\":true",
        "\"mux\":\"hybrid\"",
        "\"secureBoot\":true",
        "\"arch\":{",
        "\"opensuse\":{",
    ] {
        assert!(j.contains(k), "missing {k} in {j}");
    }
}
