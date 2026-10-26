use borea_core::*;
use std::collections::{BTreeMap, BTreeSet};
pub fn plan(d: Distribution, desktop: bool) -> Plan {
    Plan {
        version: 1,
        net: NetChoice::Ethernet,
        locale: LocaleChoice {
            lang: "en_GB.UTF-8".into(),
            keyboard: Keyboard { layout: "gb".into(), variant: "".into(), console: "uk".into() },
            timezone: "Europe/London".into(),
        },
        distro: d,
        boot_arguments: Vec::new(),
        release: match d {
            Distribution::Arch => "rolling",
            Distribution::Debian => "trixie",
            Distribution::Fedora => "43",
            Distribution::OpenSuse => "tumbleweed",
        }
        .into(),
        desktop: desktop.then_some(Desktop::Plasma),
        env: if desktop { "workstation" } else { "minimal" }.into(),
        addons: BTreeSet::new(),
        picks: Vec::new(),
        apps: BTreeSet::new(),
        prefer: SourcePreference::Native,
        sources: BTreeMap::new(),
        disk: DiskPlan::Whole {
            disk: DiskId {
                logical_sector: 512,
                path: "/dev/vda".into(),
                serial: Some("BOREA-TEST-DISK".into()),
                size: 100 * GIB,
                model: "Disposable VM disk".into(),
            },
        },
        user: Identity {
            full_name: "Test User".into(),
            username: "tester".into(),
            hostname: "borea-test".into(),
            same_passphrase: true,
        },
    }
}
