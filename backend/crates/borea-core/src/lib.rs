#![forbid(unsafe_code)]
//! Pure, owned installation intent. Secrets and observed facts are deliberately separate.
use borea_catalog::{self as catalog, Distro, Prefer, Source, Sources};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use zeroize::Zeroizing;

pub use borea_catalog::{Distro as Distribution, Prefer as SourcePreference};
pub const GIB: u64 = 1024 * 1024 * 1024;
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Plan {
    pub version: u32,
    pub net: NetChoice,
    pub locale: LocaleChoice,
    pub distro: Distro,
    pub release: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub secure_boot: Option<bool>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub boot_arguments: Vec<String>,
    pub desktop: Option<Desktop>,
    pub env: String,
    pub addons: BTreeSet<String>,
    pub picks: Vec<PackagePick>,
    pub apps: BTreeSet<String>,
    pub prefer: Prefer,
    pub sources: BTreeMap<String, bool>,
    pub disk: DiskPlan,
    pub user: Identity,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum NetChoice {
    Ethernet,
    Wifi { ssid: String },
    Hidden { ssid: String },
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LocaleChoice {
    pub lang: String,
    pub keyboard: Keyboard,
    pub timezone: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Keyboard {
    pub layout: String,
    pub variant: String,
    pub console: String,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Desktop {
    Plasma,
    Gnome,
    Cosmic,
    Budgie,
}
impl Desktop {
    pub fn key(self) -> &'static str {
        match self {
            Self::Plasma => "plasma",
            Self::Gnome => "gnome",
            Self::Cosmic => "cosmic",
            Self::Budgie => "budgie",
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PackagePick {
    pub group: String,
    pub key: String,
    pub on: bool,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DiskId {
    pub logical_sector: u64,
    pub path: PathBuf,
    pub serial: Option<String>,
    pub size: u64,
    pub model: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PartId {
    pub path: PathBuf,
    pub uuid: String,
    pub number: u32,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum DiskPlan {
    Whole { disk: DiskId },
    Alongside { disk: DiskId, windows_part: PartId, linux_size: u64 },
}
impl DiskPlan {
    pub fn disk(&self) -> &DiskId {
        match self {
            Self::Whole { disk } | Self::Alongside { disk, .. } => disk,
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Identity {
    pub full_name: String,
    pub username: String,
    pub hostname: String,
    pub same_passphrase: bool,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Step {
    Network,
    Locale,
    Distro,
    Desktop,
    System,
    Disk,
    You,
    Review,
    Install,
    Done,
    Recover,
}
impl Step {
    pub fn next(self, p: &Plan) -> Self {
        match self {
            Self::Network => Self::Locale,
            Self::Locale => Self::Distro,
            Self::Distro => {
                if p.desktop.is_some() {
                    Self::Desktop
                } else {
                    Self::System
                }
            }
            Self::Desktop => Self::System,
            Self::System => Self::Disk,
            Self::Disk => Self::You,
            Self::You => Self::Review,
            Self::Review => Self::Install,
            Self::Install => Self::Done,
            _ => self,
        }
    }
    pub fn prev(self, p: &Plan) -> Self {
        match self {
            Self::Locale => Self::Network,
            Self::Distro => Self::Locale,
            Self::Desktop => Self::Distro,
            Self::System => {
                if p.desktop.is_some() {
                    Self::Desktop
                } else {
                    Self::Distro
                }
            }
            Self::Disk => Self::System,
            Self::You => Self::Disk,
            Self::Review => Self::You,
            _ => self,
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, thiserror::Error)]
#[error("{message}")]
pub struct Hint {
    pub field: String,
    pub message: String,
}
pub fn hint(field: &str, message: impl Into<String>) -> Hint {
    Hint { field: field.into(), message: message.into() }
}
/// A secret intentionally cannot be serialized, cloned, or displayed.
pub struct Secret(Zeroizing<String>);
impl Secret {
    pub fn new(value: String) -> Self {
        Self(Zeroizing::new(value))
    }
    pub fn expose(&self) -> &str {
        self.0.as_str()
    }
}
impl std::fmt::Debug for Secret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("[REDACTED]")
    }
}
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Secrets {
    pub wifi: Option<Secret>,
    pub passphrase: Secret,
    pub admin: Option<Secret>,
}
impl Secrets {
    pub fn validate(&self, identity: &Identity) -> Result<(), Hint> {
        for secret in std::iter::once(&self.passphrase).chain(self.admin.iter()) {
            let s = secret.expose();
            if !(8..=1024).contains(&s.chars().count())
                || s.chars().any(|c| c == '\n' || c == '\r' || c == '\0')
            {
                return Err(hint(
                    "passphrase",
                    "Use between eight and 1024 characters without line breaks.",
                ));
            }
        }
        if !identity.same_passphrase && self.admin.is_none() {
            return Err(hint("admin", "Enter the separate administrator passphrase."));
        }
        Ok(())
    }
}
impl Identity {
    pub fn validate(&self) -> Result<(), Hint> {
        if !valid_username(&self.username) {
            return Err(hint("username", "Choose a valid, non-reserved Linux username."));
        }
        if !valid_hostname(&self.hostname) {
            return Err(hint(
                "hostname",
                "Use a computer name containing lowercase letters, digits and interior hyphens.",
            ));
        }
        if self.full_name.len() > 256 || self.full_name.chars().any(|c| c.is_control() || c == ':')
        {
            return Err(hint(
                "full_name",
                "The full name cannot contain control characters or a colon.",
            ));
        }
        Ok(())
    }
}
pub fn valid_username(name: &str) -> bool {
    let mut chars = name.chars();
    !name.is_empty()
        && name.len() <= 32
        && chars.next().is_some_and(|c| c.is_ascii_lowercase() || c == '_')
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-')
        && ![
            "root",
            "daemon",
            "bin",
            "sys",
            "sync",
            "games",
            "man",
            "lp",
            "mail",
            "news",
            "uucp",
            "proxy",
            "www-data",
            "backup",
            "list",
            "irc",
            "nobody",
            "systemd-network",
            "systemd-resolve",
            "borea-build",
        ]
        .contains(&name)
}
pub fn valid_hostname(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 63
        && !s.starts_with('-')
        && !s.ends_with('-')
        && s.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}
pub fn safe_value(s: &str) -> bool {
    !s.is_empty() && !s.starts_with('-') && !s.chars().any(|c| c.is_control())
}
pub fn source(key: &str, d: Distro) -> Result<Source, Hint> {
    match key {
        "aur" if d == Distro::Arch => Ok(Source::Aur),
        "flathub" => Ok(Source::Flathub),
        "appimage" => Ok(Source::AppImage),
        _ => catalog::source::repo(key)
            .filter(|r| r.distro == d)
            .map(|r| Source::Repo(r.id))
            .ok_or_else(|| {
                hint("sources", format!("Source {key} is not available for {}.", d.name()))
            }),
    }
}
impl Plan {
    pub fn target_secure_boot(&self, hw: &catalog::Hardware) -> bool {
        self.secure_boot.unwrap_or(hw.secure_boot)
    }
    pub fn validate_firmware(&self, hw: &catalog::Hardware) -> Result<(), Hint> {
        let target = self.target_secure_boot(hw);
        if !target && hw.secure_boot {
            return Err(hint(
                "secure_boot",
                "Turn off Secure Boot in firmware before requesting an unsigned installation.",
            ));
        }
        if self.distro == Distro::Arch && target && hw.setup_mode != Some(true) {
            return Err(hint(
                "secure_boot",
                "Preparing signed Arch boot requires firmware Setup Mode; reset keys yourself or choose Secure Boot off.",
            ));
        }
        Ok(())
    }
    pub fn validate_intent(&self) -> Result<(), Hint> {
        if self.version != 1 {
            return Err(hint("version", "This installation plan version is not supported."));
        }
        self.user.validate()?;
        if self.boot_arguments.len() > 16 || self.boot_arguments.iter().any(|arg| arg.len() > 128) {
            return Err(hint("boot", "Too many or overly long boot arguments."));
        }
        for arg in &self.boot_arguments {
            let (key, value) = arg.split_once('=').unwrap_or((arg, ""));
            if !["console", "loglevel", "quiet", "splash"].contains(&key)
                || !value.bytes().all(|c| c.is_ascii_alphanumeric() || b",._-".contains(&c))
            {
                return Err(hint("boot", "Unsupported boot console or display argument."));
            }
        }
        let valid_release = match self.distro {
            Distro::Arch => self.release == "rolling",
            Distro::Debian => self.release == "trixie",
            Distro::Fedora => self.release.parse::<u32>().is_ok_and(|n| (40..=99).contains(&n)),
            Distro::OpenSuse => self.release == "tumbleweed",
        };
        if !valid_release {
            return Err(hint(
                "release",
                "Choose a supported upstream release for this distribution.",
            ));
        }
        if let NetChoice::Wifi { ssid } | NetChoice::Hidden { ssid } = &self.net {
            if ssid.is_empty() || ssid.len() > 32 || ssid.contains('\0') {
                return Err(hint("ssid", "Choose a valid network name."));
            }
        }
        for s in [
            &self.locale.lang,
            &self.locale.keyboard.layout,
            &self.locale.keyboard.console,
            &self.locale.timezone,
        ] {
            if !safe_value(s) {
                return Err(hint("locale", "Choose a valid locale, keyboard and time zone."));
            }
        }
        if !self.locale.keyboard.variant.is_empty() && !safe_value(&self.locale.keyboard.variant) {
            return Err(hint("keyboard", "Choose a valid keyboard variant."));
        }
        if self.locale.lang.contains('/')
            || self.locale.timezone.starts_with('/')
            || self.locale.timezone.split('/').any(|p| p == ".." || p.is_empty())
        {
            return Err(hint("locale", "Choose a system locale and time zone from the catalog."));
        }
        self.validate_system()?;
        let id = self.disk.disk();
        if !id.path.is_absolute()
            || !id.path.starts_with("/dev")
            || id.path.components().any(|c| matches!(c, std::path::Component::ParentDir))
            || id.size == 0
        {
            return Err(hint("disk", "Choose a real disk by its stable identity."));
        }
        Ok(())
    }
    pub fn validate_system(&self) -> Result<(), Hint> {
        SystemChoice::from(self).validate()
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SystemChoice {
    pub distro: Distro,
    pub desktop: Option<Desktop>,
    pub env: String,
    pub addons: BTreeSet<String>,
    pub picks: Vec<PackagePick>,
    pub apps: BTreeSet<String>,
    pub sources: BTreeMap<String, bool>,
}
impl From<&Plan> for SystemChoice {
    fn from(plan: &Plan) -> Self {
        Self {
            distro: plan.distro,
            desktop: plan.desktop,
            env: plan.env.clone(),
            addons: plan.addons.clone(),
            picks: plan.picks.clone(),
            apps: plan.apps.clone(),
            sources: plan.sources.clone(),
        }
    }
}
impl SystemChoice {
    pub fn validate(&self) -> Result<(), Hint> {
        let env = catalog::group(&self.env)
            .filter(|g| g.kind == catalog::Kind::Environment)
            .ok_or_else(|| hint("env", "Choose a base system profile."))?;
        if env.desktop != self.desktop.is_some() {
            return Err(hint(
                "desktop",
                "The desktop choice must match the selected system profile.",
            ));
        }
        let mut seen = BTreeSet::new();
        let mut picks = catalog::Picks::default();
        for p in &self.picks {
            if !seen.insert((&p.group, &p.key)) {
                return Err(hint("picks", "A package choice appears more than once."));
            }
            if p.group != self.env && !self.addons.contains(&p.group) {
                return Err(hint(
                    "picks",
                    "Package overrides must belong to a selected profile or add-on.",
                ));
            }
            if !catalog::group(&p.group)
                .is_some_and(|group| group.offers(self.desktop.is_some()).any(|key| key == p.key))
            {
                return Err(hint(
                    "picks",
                    "This package choice requires a desktop or is not in the selected group.",
                ));
            }
            picks
                .set(&p.group, &p.key, p.on)
                .map_err(|e| hint("picks", format!("Invalid package selection: {e:?}")))?;
        }
        let addons = self.addons.iter().map(String::as_str).collect::<Vec<_>>();
        catalog::plan_groups_with(
            &self.env,
            &addons,
            &picks,
            self.desktop.map_or("", Desktop::key),
            self.distro,
        )
        .map_err(|e| match e {
            catalog::GroupError::UnavailableDesktop(reason) => hint("desktop", reason),
            _ => hint("env", format!("Invalid profile: {e:?}")),
        })?;
        if !env.allows_apps && !self.apps.is_empty() {
            return Err(hint(
                "apps",
                "This profile has no desktop; clear desktop apps before continuing.",
            ));
        }
        for id in &self.apps {
            if catalog::app(id).is_none() {
                return Err(hint("apps", format!("Unknown app: {id}.")));
            }
        }
        for key in self.sources.keys() {
            source(key, self.distro)?;
        }
        Ok(())
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Package {
    pub source: String,
    pub name: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Review {
    pub distro: Distro,
    pub release: String,
    pub environment: String,
    pub desktop: Option<Desktop>,
    pub packages: Vec<Package>,
    pub apps: Vec<String>,
    pub estimated_app_mb: u64,
    pub firewall: Vec<String>,
    pub services: Vec<String>,
    pub kernel_args: Vec<String>,
    pub modprobe: Vec<String>,
    pub warnings: Vec<String>,
    pub mok: bool,
}
/// The job consumes this exact resolved package list; no second resolver exists.
pub fn review(plan: &Plan, hw: &catalog::Hardware) -> Result<Review, Hint> {
    plan.validate_intent()?;
    let mut target_hw = hw.clone();
    target_hw.secure_boot = plan.target_secure_boot(hw);
    let driver = catalog::plan_drivers(&target_hw, plan.distro);
    let mut sources = Sources::defaults(plan.distro);
    for (key, on) in &plan.sources {
        sources
            .set(source(key, plan.distro)?, *on)
            .map_err(|e| hint("sources", format!("Invalid source choice: {e:?}")))?;
    }
    sources
        .require(&driver.sources)
        .map_err(|e| hint("drivers", format!("Cannot enable driver sources: {e:?}")))?;
    let mut picks = catalog::Picks::default();
    for p in &plan.picks {
        picks
            .set(&p.group, &p.key, p.on)
            .map_err(|e| hint("picks", format!("Invalid package selection: {e:?}")))?;
    }
    let addons = plan.addons.iter().map(String::as_str).collect::<Vec<_>>();
    let groups = catalog::plan_groups_with(
        &plan.env,
        &addons,
        &picks,
        plan.desktop.map_or("", Desktop::key),
        plan.distro,
    )
    .map_err(|e| hint("env", format!("Invalid profile: {e:?}")))?;
    let app_ids =
        plan.apps.iter().map(String::as_str).chain(groups.apps.iter().copied()).collect::<Vec<_>>();
    let selected = catalog::Selection::with(&app_ids);
    let app_plan = catalog::plan(&selected, &sources, plan.prefer);
    if !app_plan.stranded.is_empty() {
        return Err(hint(
            "apps",
            format!("Enable a source for: {}.", app_plan.stranded.join(", ")),
        ));
    }
    let base = catalog::repos_for(plan.distro)
        .find(|r| r.base)
        .ok_or_else(|| hint("sources", "This target has no base repository."))?;
    let mut packages = BTreeSet::new();
    for pkg in groups.packages {
        let source = if plan.distro == Distro::Debian && pkg == "firmware-linux" {
            "debian.non-free-firmware"
        } else {
            base.id
        };
        packages.insert((source.to_owned(), pkg.to_owned()));
    }
    if let Some((desktop, manager)) = groups.desktop {
        packages.insert((base.id.into(), desktop.into()));
        packages.insert((base.id.into(), manager.into()));
    }
    for p in &driver.packages {
        packages.insert((p.source.key().into(), p.name.into()));
    }
    for (src, pkgs) in app_plan.batches {
        for p in pkgs {
            packages.insert((src.key().into(), p.into()));
        }
    }
    let mut services = driver.services.iter().map(|s| (*s).to_owned()).collect::<BTreeSet<_>>();
    services.insert("NetworkManager.service".into());
    if let Some((_, manager)) = groups.desktop {
        services.insert(format!("{manager}.service"));
    }
    let names = packages.iter().map(|(_, name)| name.as_str()).collect::<Vec<_>>();
    services
        .extend(catalog::groups::role_services(plan.distro, &names).into_iter().map(str::to_owned));

    Ok(Review {
        distro: plan.distro,
        release: plan.release.clone(),
        environment: plan.env.clone(),
        desktop: plan.desktop,
        packages: packages.into_iter().map(|(source, name)| Package { source, name }).collect(),
        apps: app_ids.into_iter().map(str::to_owned).collect::<BTreeSet<_>>().into_iter().collect(),
        estimated_app_mb: u64::from(app_plan.size_mb),
        firewall: groups.firewall.into_iter().map(str::to_owned).collect(),
        services: services.into_iter().collect(),
        kernel_args: driver
            .kernel_args
            .iter()
            .map(|s| (*s).into())
            .chain(plan.boot_arguments.iter().cloned())
            .collect(),
        modprobe: driver.modprobe.iter().map(|s| (*s).into()).collect(),
        warnings: driver.warning.into_iter().collect(),
        mok: driver.mok,
    })
}

impl<'de> Deserialize<'de> for Secret {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        String::deserialize(deserializer).map(Self::new)
    }
}
