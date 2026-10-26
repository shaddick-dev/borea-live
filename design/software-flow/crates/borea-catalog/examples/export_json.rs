#![forbid(unsafe_code)]
//! `cargo run --example export_json > catalog.json`
//! Dumps the catalogue so the design prototype uses exactly the same data as the installer.

use borea_catalog::*;

fn s(x: &str) -> String {
    format!("\"{}\"", x.replace('\\', "\\\\").replace('"', "\\\""))
}

fn opt(x: Option<&str>) -> String {
    x.map(s).unwrap_or_else(|| "null".into())
}

fn main() {
    let repos: Vec<String> = REPOS
        .iter()
        .map(|r| {
            format!(
                "{{\"id\":{},\"distro\":{},\"label\":{},\"trust\":{},\"base\":{},\"defaultOn\":{},\"onDemand\":{},\"note\":{}}}",
                s(r.id), s(r.distro.key()), s(r.label),
                s(if r.trust == Trust::Official { "official" } else { "community" }),
                r.base, r.default_on, r.on_demand, s(r.note)
            )
        })
        .collect();

    let apps: Vec<String> = APPS
        .iter()
        .map(|a| {
            let native: Vec<String> = a
                .native
                .iter()
                .map(|n| format!("{{\"distro\":{},\"pkg\":{},\"repo\":{}}}", s(n.distro.key()), s(n.pkg), s(n.repo)))
                .collect();
            format!(
                "{{\"id\":{},\"name\":{},\"what\":{},\"cat\":{},\"mb\":{},\"native\":[{}],\"aur\":{},\"flatpak\":{},\"appimage\":{},\"listed\":{}}}",
                s(a.id), s(a.name), s(a.what), s(a.category.label()), a.mb, native.join(","), opt(a.aur), opt(a.flatpak), opt(a.appimage), listed(a)
            )
        })
        .collect();

    let cats: Vec<String> = Category::ALL.iter().map(|c| s(c.label())).collect();
    let presets: Vec<String> = PRESETS
        .iter()
        .map(|(name, ids, addons)| {
            format!(
                "{{\"name\":{},\"apps\":[{}],\"addons\":[{}]}}",
                s(name),
                ids.iter().map(|i| s(i)).collect::<Vec<_>>().join(","),
                addons.iter().map(|i| s(i)).collect::<Vec<_>>().join(",")
            )
        })
        .collect();

    let list = |xs: &[&str]| xs.iter().map(|x| s(x)).collect::<Vec<_>>().join(",");
    let groups: Vec<String> = GROUPS
        .iter()
        .map(|g| {
            format!(
                "{{\"id\":{},\"name\":{},\"what\":{},\"kind\":{},\"section\":{},\"desktop\":{},\"allowsApps\":{},\"items\":[{}],\"guiItems\":[{}],\"optional\":[{}],\"apps\":[{}]}}",
                s(g.id), s(g.name), s(g.what), s(if g.kind == Kind::Environment { "env" } else { "addon" }), s(g.section),
                g.desktop, g.allows_apps, list(g.items), list(g.gui_items), list(g.optional), list(g.apps)
            )
        })
        .collect();
    let pkgs: Vec<String> = PKGS
        .iter()
        .map(|(k, n)| format!("{}:[{}]", s(k), list(n)))
        .collect();
    let desktops: Vec<String> = DESKTOPS
        .iter()
        .map(|(k, per)| {
            format!(
                "{}:[{}]",
                s(k),
                per.iter()
                    .map(|(p, dm)| format!("[{},{}]", s(p), s(dm)))
                    .collect::<Vec<_>>()
                    .join(",")
            )
        })
        .collect();

    // Driver plans for the sample machines, per distro: the prototype's "simulate hardware" switch.
    let drivers: Vec<String> = hardware::samples()
        .iter()
        .map(|(id, hw)| {
            let per: Vec<String> = Distro::ALL
                .iter()
                .map(|d| {
                    let p = plan_drivers(hw, *d);
                    let found: Vec<String> = p.found.iter().map(|f| format!("[{},{}]", s(&f.what), s(&f.how))).collect();
                    let pk: Vec<String> = p.packages.iter().map(|x| format!("[{},{}]", s(x.source.key()), s(x.name))).collect();
                    let src: Vec<String> = p.sources.iter().map(|x| s(x.key())).collect();
                    format!(
                        "{}:{{\"found\":[{}],\"packages\":[{}],\"sources\":[{}],\"services\":[{}],\"kernelArgs\":[{}],\"mok\":{},\"warning\":{}}}",
                        s(d.key()), found.join(","), pk.join(","), src.join(","), list(&p.services), list(&p.kernel_args), p.mok, opt(p.warning.as_deref())
                    )
                })
                .collect();
            format!("{}:{{\"name\":{},\"laptop\":{},{}}}", s(id), s(&hw.display_name()), hw.is_laptop(), per.join(","))
        })
        .collect();

    println!(
        "{{\"repos\":[{}],\"categories\":[{}],\"apps\":[{}],\"presets\":[{}],\"groups\":[{}],\"pkgs\":{{{}}},\"desktops\":{{{}}},\"drivers\":{{{}}},\"required\":[{}],\"sections\":[{}],\"firewall\":{{{}}}}}",
        repos.join(","),
        cats.join(","),
        apps.join(","),
        presets.join(","),
        groups.join(","),
        pkgs.join(","),
        desktops.join(","),
        drivers.join(","),
        list(REQUIRED),
        list(SECTIONS),
        FIREWALL
            .iter()
            .map(|(k, r)| format!("{}:[{}]", s(k), list(r)))
            .collect::<Vec<_>>()
            .join(",")
    );
}
