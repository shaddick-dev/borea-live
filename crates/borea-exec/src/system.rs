//! `borea` on an installed system: add and remove apps, extras or any package,
//! with the distribution's own package manager, the AUR on Arch, or Flathub.
//! Borea records what it added, so `borea remove` takes away only that.
use crate::postinstall::persist;
use borea_catalog::{Distro, Prefer, Resolution, Source, Sources};
use borea_core::{Hint, hint};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::process::Command;

const RECORD: &str = "/var/lib/borea/apps.json";
const FLATHUB: &str = "https://dl.flathub.org/repo/flathub.flatpakrepo";

fn err(message: impl Into<String>) -> Hint {
    hint("borea", message)
}

/// The installed family, from /etc/os-release's ID and ID_LIKE.
pub fn distro(os_release: &str) -> Option<Distro> {
    let field = |key: &str| {
        os_release
            .lines()
            .find_map(|l| l.strip_prefix(key)?.strip_prefix('='))
            .map(|v| v.trim_matches('"').to_ascii_lowercase())
            .unwrap_or_default()
    };
    let ids = format!("{} {}", field("ID"), field("ID_LIKE"));
    ids.split_whitespace().find_map(|id| match id {
        "arch" | "cachyos" => Some(Distro::Arch),
        "opensuse" | "opensuse-tumbleweed" | "suse" => Some(Distro::OpenSuse),
        "fedora" => Some(Distro::Fedora),
        "debian" | "ubuntu" => Some(Distro::Debian),
        _ => None,
    })
}
fn this_system() -> Result<Distro, Hint> {
    distro(&fs::read_to_string("/etc/os-release").unwrap_or_default())
        .ok_or_else(|| err("This system's distribution is not one Borea installs."))
}

/// Where one name comes from on this system.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "from", rename_all = "lowercase")]
pub enum Origin {
    /// The distribution's own repositories (including Packman, OBS or multilib).
    Native {
        packages: Vec<String>,
    },
    Aur {
        package: String,
    },
    Flathub {
        id: String,
    },
}

/// Which source an install may use: any (Borea chooses), or only one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Pin {
    #[default]
    Any,
    /// `-n`: this system's own repositories.
    Native,
    /// `-a`: the AUR (Arch only).
    Aur,
    /// `-f`: Flathub.
    Flatpak,
}

/// What a name means, in order: a Borea extra (gaming, codecs, printing,
/// flatpak), an app from Borea's catalog by id or name, then any package the
/// system's own repositories have, then the AUR on Arch, then a Flathub app id.
pub fn resolve(
    name: &str,
    distro: Distro,
    native: &dyn Fn(&str) -> bool,
    aur: &dyn Fn(&str) -> bool,
    flathub: &dyn Fn(&str) -> bool,
) -> Result<Origin, Hint> {
    resolve_pinned(name, distro, Pin::Any, native, aur, flathub)
}
/// As `resolve`, keeping to one source when pinned. A catalog app pinned to a
/// source uses that source's package or app id; any other name must exist there.
pub fn resolve_pinned(
    name: &str,
    distro: Distro,
    pin: Pin,
    native: &dyn Fn(&str) -> bool,
    aur: &dyn Fn(&str) -> bool,
    flathub: &dyn Fn(&str) -> bool,
) -> Result<Origin, Hint> {
    if pin == Pin::Aur && distro != Distro::Arch {
        return Err(err("The AUR is only for Arch and CachyOS."));
    }
    if pin != Pin::Any {
        let wanted = name.to_ascii_lowercase();
        if let Some(app) = borea_catalog::APPS
            .iter()
            .find(|a| a.id == wanted || a.name.to_ascii_lowercase() == wanted)
        {
            let found = borea_catalog::resolve::candidates(app, distro, Prefer::Native)
                .into_iter()
                .find(|(source, _)| match pin {
                    Pin::Native => matches!(source, Source::Repo(_)),
                    Pin::Aur => *source == Source::Aur,
                    _ => *source == Source::Flathub,
                });
            return match found {
                Some((Source::Repo(_), p)) => Ok(Origin::Native { packages: vec![p.into()] }),
                Some((Source::Aur, p)) => Ok(Origin::Aur { package: p.into() }),
                Some((_, id)) => Ok(Origin::Flathub { id: id.into() }),
                None => Err(err(format!("{} isn't offered there for this system.", app.name))),
            };
        }
        if !borea_backend::external::identifier(name) {
            return Err(err(format!("{name} is not a package name.")));
        }
        return match pin {
            Pin::Native if native(name) => Ok(Origin::Native { packages: vec![name.into()] }),
            Pin::Aur if aur(name) => Ok(Origin::Aur { package: name.into() }),
            Pin::Flatpak if name.contains('.') && flathub(name) => {
                Ok(Origin::Flathub { id: name.into() })
            }
            Pin::Flatpak if !name.contains('.') => Err(err(format!(
                "{name} isn't in Borea's catalog. Give its Flathub app id (try `borea s {name}`)."
            ))),
            _ => Err(err(format!("{name} isn't there."))),
        };
    }
    if let Some(packages) = borea_catalog::groups::extra_packages(distro, name) {
        return Ok(Origin::Native {
            packages: packages.into_iter().map(|(_, p)| p.to_owned()).collect(),
        });
    }
    let wanted = name.to_ascii_lowercase();
    if let Some(app) =
        borea_catalog::APPS.iter().find(|a| a.id == wanted || a.name.to_ascii_lowercase() == wanted)
    {
        let prefer = if distro == Distro::Arch { Prefer::Aur } else { Prefer::Native };
        let (Resolution::From { source, package } | Resolution::NeedsSource { source, package }) =
            borea_catalog::resolve(app, &Sources::defaults(distro), prefer)
        else {
            return Err(err(format!("{} is not available for this system.", app.name)));
        };
        return Ok(match source {
            Source::Aur => Origin::Aur { package: package.into() },
            Source::Flathub => Origin::Flathub { id: package.into() },
            Source::Repo(_) => Origin::Native { packages: vec![package.into()] },
            Source::AppImage => {
                return Err(err(format!("{} is only published as an AppImage.", app.name)));
            }
        });
    }
    if !borea_backend::external::identifier(name) {
        return Err(err(format!("{name} is not a package name.")));
    }
    if native(name) {
        return Ok(Origin::Native { packages: vec![name.into()] });
    }
    if distro == Distro::Arch && aur(name) {
        return Ok(Origin::Aur { package: name.into() });
    }
    if name.contains('.') && flathub(name) {
        return Ok(Origin::Flathub { id: name.into() });
    }
    Err(err(format!(
        "{name} is not in this system's repositories{}, or on Flathub.",
        if distro == Distro::Arch { ", the AUR" } else { "" }
    )))
}

fn run(program: &str, args: &[&str]) -> Result<(), Hint> {
    let status = Command::new(program)
        .args(args)
        .env("DEBIAN_FRONTEND", "noninteractive")
        .status()
        .map_err(|e| err(format!("{program}: {e}")))?;
    status.success().then_some(()).ok_or_else(|| err(format!("{program} failed.")))
}
fn quiet(program: &str, args: &[&str]) -> bool {
    Command::new(program).args(args).env("LC_ALL", "C").output().is_ok_and(|o| o.status.success())
}
/// Whether the system's own repositories have this package.
fn in_repositories(distro: Distro, name: &str) -> bool {
    match distro {
        Distro::Arch => quiet("/usr/bin/pacman", &["-Si", "--", name]),
        Distro::OpenSuse => Command::new("/usr/bin/zypper")
            .args(["--non-interactive", "--no-refresh", "info", name])
            .env("LC_ALL", "C")
            .output()
            .is_ok_and(|o| {
                String::from_utf8_lossy(&o.stdout).lines().any(|l| l.starts_with("Name"))
            }),
        Distro::Fedora => quiet("/usr/bin/dnf", &["-q", "info", name]),
        Distro::Debian => quiet("/usr/bin/apt-cache", &["show", name]),
    }
}
fn native_install(distro: Distro, packages: &[String]) -> Result<(), Hint> {
    let mut args: Vec<&str> = match distro {
        Distro::Arch => vec!["-S", "--needed", "--noconfirm", "--"],
        Distro::OpenSuse => vec!["--non-interactive", "install", "--auto-agree-with-licenses"],
        Distro::Fedora => vec!["-y", "install"],
        Distro::Debian => vec!["-y", "install"],
    };
    args.extend(packages.iter().map(String::as_str));
    run(manager(distro), &args)
}
fn native_remove(distro: Distro, packages: &[String]) -> Result<(), Hint> {
    let mut args: Vec<&str> = match distro {
        Distro::Arch => vec!["-Rns", "--noconfirm", "--"],
        Distro::OpenSuse => vec!["--non-interactive", "remove", "--clean-deps"],
        Distro::Fedora => vec!["-y", "remove"],
        Distro::Debian => vec!["-y", "remove", "--autoremove"],
    };
    args.extend(packages.iter().map(String::as_str));
    run(manager(distro), &args)
}
fn manager(distro: Distro) -> &'static str {
    match distro {
        Distro::Arch => "/usr/bin/pacman",
        Distro::OpenSuse => "/usr/bin/zypper",
        Distro::Fedora => "/usr/bin/dnf",
        Distro::Debian => "/usr/bin/apt-get",
    }
}

/// Build an AUR package and its AUR dependencies as Borea's unprivileged build
/// account, each archive pinned to the digest read before building.
fn aur_install(package: &str) -> Result<(), Hint> {
    let recipes = borea_backend::external::aur_recipes(package)?;
    let cache = Path::new("/var/lib/borea/aur");
    fs::create_dir_all(cache).map_err(|e| err(e.to_string()))?;
    for recipe in &recipes {
        crate::download(
            &recipe.url,
            &cache.join(format!("{}.tar.gz", recipe.base)),
            Some(&recipe.sha256),
        )?;
    }
    crate::aur::install(
        Path::new("/"),
        cache,
        &recipes,
        crate::postinstall::aur_command,
        |_| Ok(()),
    )
}
fn flatpak_install(distro: Distro, id: &str) -> Result<(), Hint> {
    borea_backend::external::flathub(id)?;
    if !Path::new("/usr/bin/flatpak").is_file() {
        native_install(distro, &["flatpak".into()])?;
    }
    run(
        "/usr/bin/flatpak",
        &["remote-add", "--system", "--noninteractive", "--if-not-exists", "flathub", FLATHUB],
    )?;
    run("/usr/bin/flatpak", &["install", "--system", "--noninteractive", "-y", "flathub", id])
}

/// What Borea added: name → where it came from.
pub fn record() -> BTreeMap<String, Origin> {
    fs::read(RECORD).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
}
pub(crate) fn remember(name: &str, origin: Origin) -> Result<(), Hint> {
    let mut all = record();
    all.insert(name.to_ascii_lowercase(), origin);
    let path = Path::new(RECORD);
    fs::create_dir_all(path.parent().ok_or_else(|| err("No state directory."))?)
        .map_err(|e| err(e.to_string()))?;
    persist(path, &serde_json::to_vec_pretty(&all).map_err(|e| err(e.to_string()))?, 0o644)
}

fn lookup(distro: Distro, name: &str, pin: Pin) -> Result<Origin, Hint> {
    resolve_pinned(
        name,
        distro,
        pin,
        &|n| in_repositories(distro, n),
        &|n| borea_backend::external::verify_source("aur", n).is_ok(),
        &|n| borea_backend::external::flathub(n).is_ok(),
    )
}
fn install_one(distro: Distro, name: &str, pin: Pin) -> Result<Origin, Hint> {
    let origin = resolve_pinned(
        name,
        distro,
        pin,
        &|n| in_repositories(distro, n),
        &|n| borea_backend::external::verify_source("aur", n).is_ok(),
        &|n| borea_backend::external::flathub(n).is_ok(),
    )?;
    match &origin {
        Origin::Native { packages } => native_install(distro, packages)?,
        Origin::Aur { package } => aur_install(package)?,
        Origin::Flathub { id } => flatpak_install(distro, id)?,
    }
    remember(name, origin.clone())?;
    Ok(origin)
}
fn describe(origin: &Origin) -> String {
    match origin {
        Origin::Native { packages } => {
            format!("from the system's repositories ({})", packages.join(", "))
        }
        Origin::Aur { package } => format!("from the AUR ({package})"),
        Origin::Flathub { id } => format!("from Flathub ({id})"),
    }
}

/// `borea in [-n|-a|-f] NAME…`
pub fn install(names: &[String], pin: Pin) -> Result<(), Hint> {
    let distro = this_system()?;
    let mut failed = Vec::new();
    for name in names {
        match install_one(distro, name, pin) {
            Ok(origin) => println!("Installed {name} {}.", describe(&origin)),
            Err(e) => {
                eprintln!("{name}: {}", e.message);
                failed.push(name.as_str());
            }
        }
    }
    failed
        .is_empty()
        .then_some(())
        .ok_or_else(|| err(format!("Not installed: {}.", failed.join(", "))))
}
/// `borea remove NAME…`: only what Borea added.
pub fn remove(names: &[String]) -> Result<(), Hint> {
    let distro = this_system()?;
    let mut all = record();
    for name in names {
        let key = name.to_ascii_lowercase();
        let origin = all.get(&key).cloned().ok_or_else(|| {
            err(format!("Borea didn't add {name}, so it leaves it alone. Remove it with the system's package manager."))
        })?;
        match &origin {
            Origin::Native { packages } => native_remove(distro, packages)?,
            Origin::Aur { package } => native_remove(Distro::Arch, std::slice::from_ref(package))?,
            Origin::Flathub { id } => {
                run("/usr/bin/flatpak", &["uninstall", "--system", "--noninteractive", "-y", id])?
            }
        }
        all.remove(&key);
        persist(
            Path::new(RECORD),
            &serde_json::to_vec_pretty(&all).map_err(|e| err(e.to_string()))?,
            0o644,
        )?;
        println!("Removed {name}.");
    }
    Ok(())
}
/// `borea which NAME…`: where each would come from, installing nothing.
pub fn which(names: &[String], pin: Pin) -> Result<(), Hint> {
    let distro = this_system()?;
    let added = record();
    for name in names {
        match lookup(distro, name, pin) {
            Ok(origin) => println!(
                "{name}: {}{}",
                describe(&origin),
                if added.contains_key(&name.to_ascii_lowercase()) {
                    " — added by Borea"
                } else {
                    ""
                }
            ),
            Err(e) => println!("{name}: {}", e.message),
        }
    }
    Ok(())
}
/// `borea s TERM`: Borea's catalog, this system's repositories, the AUR on
/// Arch, and Flathub when Flatpak is installed.
pub fn search(term: &str) -> Result<(), Hint> {
    let distro = this_system()?;
    let wanted = term.to_ascii_lowercase();
    let catalog = borea_catalog::APPS
        .iter()
        .filter(|a| {
            a.id.contains(&wanted)
                || a.name.to_ascii_lowercase().contains(&wanted)
                || a.what.to_ascii_lowercase().contains(&wanted)
        })
        .collect::<Vec<_>>();
    if !catalog.is_empty() {
        println!("Borea's catalog (borea in NAME):");
        for app in catalog {
            println!("  {:22} {}", app.id, app.what);
        }
    }
    let heading = |title: &str| println!("\n{title}:");
    let show = |program: &str, args: &[&str]| {
        let _ = Command::new(program).args(args).status();
    };
    heading("This system's repositories (borea in -n NAME)");
    match distro {
        Distro::Arch => show("/usr/bin/pacman", &["-Ss", "--", term]),
        Distro::OpenSuse => {
            show("/usr/bin/zypper", &["--non-interactive", "--no-refresh", "search", term])
        }
        Distro::Fedora => show("/usr/bin/dnf", &["-q", "search", term]),
        Distro::Debian => show("/usr/bin/apt-cache", &["search", term]),
    }
    if distro == Distro::Arch && borea_backend::external::identifier(term) {
        heading("The AUR (borea in -a NAME)");
        for (name, what) in
            borea_backend::external::aur_search(term).unwrap_or_default().into_iter().take(20)
        {
            println!("  {name:30} {what}");
        }
    }
    if Path::new("/usr/bin/flatpak").is_file() {
        heading("Flathub (borea in -f APP-ID)");
        show("/usr/bin/flatpak", &["search", "--columns=application,name,description", term]);
    }
    Ok(())
}
/// `borea up`: the system, the AUR packages Borea added, and Flatpak apps.
pub fn update() -> Result<(), Hint> {
    let distro = this_system()?;
    match distro {
        Distro::Arch => run("/usr/bin/pacman", &["-Syu", "--noconfirm"])?,
        Distro::OpenSuse => run(
            "/usr/bin/zypper",
            &["--non-interactive", "dist-upgrade", "--auto-agree-with-licenses"],
        )?,
        Distro::Fedora => run("/usr/bin/dnf", &["-y", "upgrade", "--refresh"])?,
        Distro::Debian => {
            run("/usr/bin/apt-get", &["update"])?;
            run("/usr/bin/apt-get", &["-y", "full-upgrade"])?;
        }
    }
    for (name, origin) in record() {
        if let Origin::Aur { package } = origin {
            if aur_outdated(&package) {
                println!("Updating {name} from the AUR.");
                aur_install(&package)?;
            }
        }
    }
    if Path::new("/usr/bin/flatpak").is_file() {
        run("/usr/bin/flatpak", &["update", "--system", "--noninteractive", "-y"])?;
    }
    println!("Up to date.");
    Ok(())
}
/// Whether the AUR has a different version than the one installed.
fn aur_outdated(package: &str) -> bool {
    let installed = Command::new("/usr/bin/pacman")
        .args(["-Q", "--", package])
        .output()
        .ok()
        .map(|o| {
            String::from_utf8_lossy(&o.stdout).split_whitespace().nth(1).unwrap_or("").to_owned()
        })
        .unwrap_or_default();
    borea_backend::external::aur_version(package).is_ok_and(|v| v != installed)
}

/// `borea list`
pub fn list() -> Result<(), Hint> {
    let all = record();
    if all.is_empty() {
        println!("Borea hasn't added anything to this system.");
    }
    for (name, origin) in all {
        println!("{name:24} {}", describe(&origin));
    }
    Ok(())
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic)]
mod tests {
    use super::*;
    #[test]
    fn the_installer_s_record_reads_back() {
        let written = r#"{"vesktop":{"from":"aur","package":"vesktop-bin"},"firefox":{"from":"native","packages":["firefox"]},"spotify":{"from":"flathub","id":"com.spotify.Client"}}"#;
        let all: BTreeMap<String, Origin> = serde_json::from_str(written).unwrap();
        assert_eq!(all["vesktop"], Origin::Aur { package: "vesktop-bin".into() });
        assert_eq!(all["spotify"], Origin::Flathub { id: "com.spotify.Client".into() });
    }
    #[test]
    fn pinned_sources_keep_to_their_source() {
        let no = |_: &str| false;
        let yes = |_: &str| true;
        // A catalog app pinned to Flathub uses its Flathub id, by name.
        assert_eq!(
            resolve_pinned("Vesktop", Distro::Arch, Pin::Flatpak, &no, &no, &no).unwrap(),
            Origin::Flathub { id: "dev.vencord.Vesktop".into() }
        );
        assert_eq!(
            resolve_pinned("vesktop", Distro::Arch, Pin::Aur, &no, &no, &no).unwrap(),
            Origin::Aur { package: "vesktop-bin".into() }
        );
        assert!(resolve_pinned("vesktop", Distro::OpenSuse, Pin::Aur, &no, &yes, &no).is_err());
        // Any other name must exist in the pinned source.
        assert_eq!(
            resolve_pinned("htop", Distro::OpenSuse, Pin::Native, &yes, &no, &no).unwrap(),
            Origin::Native { packages: vec!["htop".into()] }
        );
        assert!(resolve_pinned("htop", Distro::OpenSuse, Pin::Native, &no, &yes, &yes).is_err());
        assert!(
            resolve_pinned("org.example.App", Distro::Arch, Pin::Flatpak, &yes, &yes, &yes).is_ok()
        );
        let message =
            resolve_pinned("someapp", Distro::Arch, Pin::Flatpak, &no, &no, &yes).unwrap_err();
        assert!(message.message.contains("borea s someapp"));
    }
    #[test]
    fn os_release_names_the_family() {
        assert_eq!(distro("ID=cachyos\nID_LIKE=arch\n"), Some(Distro::Arch));
        assert_eq!(
            distro("ID=\"opensuse-tumbleweed\"\nID_LIKE=\"opensuse suse\"\n"),
            Some(Distro::OpenSuse)
        );
        assert_eq!(distro("ID=fedora\n"), Some(Distro::Fedora));
        assert_eq!(distro("ID=debian\n"), Some(Distro::Debian));
        assert_eq!(distro("ID=gentoo\n"), None);
    }
    #[test]
    fn names_resolve_extras_then_catalog_then_repositories_then_aur_then_flathub() {
        let no = |_: &str| false;
        let yes = |_: &str| true;
        // An extra: Tumbleweed's own package names.
        let Origin::Native { packages } =
            resolve("printing", Distro::OpenSuse, &no, &no, &no).unwrap()
        else {
            panic!("printing is native")
        };
        assert_eq!(packages, ["cups", "system-config-printer"]);
        // A catalog app by name: the AUR first on Arch, the system's own elsewhere.
        assert_eq!(
            resolve("Vesktop", Distro::Arch, &no, &no, &no).unwrap(),
            Origin::Aur { package: "vesktop-bin".into() }
        );
        assert!(matches!(
            resolve("firefox", Distro::OpenSuse, &no, &no, &no).unwrap(),
            Origin::Native { .. }
        ));
        // Any package: repositories, then the AUR (Arch only), then Flathub.
        assert_eq!(
            resolve("htop", Distro::Arch, &yes, &yes, &no).unwrap(),
            Origin::Native { packages: vec!["htop".into()] }
        );
        assert_eq!(
            resolve("yay-bin", Distro::Arch, &no, &yes, &no).unwrap(),
            Origin::Aur { package: "yay-bin".into() }
        );
        assert!(resolve("yay-bin", Distro::OpenSuse, &no, &yes, &no).is_err(), "no AUR off Arch");
        assert_eq!(
            resolve("org.gimp.GIMP.Plugin", Distro::OpenSuse, &no, &no, &yes).unwrap(),
            Origin::Flathub { id: "org.gimp.GIMP.Plugin".into() }
        );
        assert!(resolve("nothing-here", Distro::Arch, &no, &no, &no).is_err());
        assert!(resolve("../etc", Distro::Arch, &yes, &yes, &yes).is_err());
    }
}
