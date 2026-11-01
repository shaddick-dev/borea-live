//! Iced presentation. The installer workflow and workers contain no GUI types.
use super::*;
use crate::install::Page;
use crate::model::hardware_facts;
use iced::widget::{
    self, button, checkbox, column, container, image, pick_list, progress_bar, row, scrollable,
    slider, space, stack, svg, text, text_input, toggler,
};
use iced::{Alignment, Background, Border, Color, Element, Length, Subscription, Task, Theme};
use std::collections::{BTreeMap, BTreeSet};
use std::time::{Duration, Instant};
#[derive(Clone, PartialEq)]
struct Tile {
    key: String,
    label: String,
    description: String,
    meta: String,
    logo: String,
    selected: bool,
    enabled: bool,
    /// A quiet tag at the card's bottom right: why it can't be picked, or its status.
    badge: Option<String>,
    /// Facts to read rather than a choice: a flat card with no hover and no greying.
    info: bool,
}
impl Tile {
    fn new(
        key: impl Into<String>,
        label: impl Into<String>,
        description: impl Into<String>,
        selected: bool,
    ) -> Self {
        Self {
            key: key.into(),
            label: label.into(),
            description: description.into(),
            meta: String::new(),
            logo: String::new(),
            selected,
            enabled: true,
            badge: None,
            info: false,
        }
    }
}
#[derive(Clone, PartialEq)]
struct Section {
    title: String,
    choices: Vec<Tile>,
}
fn section(title: &str, choices: Vec<Tile>) -> Section {
    Section { title: title.into(), choices }
}
struct Bridge {
    app: LiveInstaller,
    network: Option<usize>,
    network_name: Option<String>,
    pending_family: Option<(Family, Instant)>,
}
impl Bridge {
    /// The family shown as picked: the one just clicked, else the one already chosen.
    fn picked(&self) -> Option<Family> {
        self.pending_family.map(|(f, _)| f).or(self.app.picked_family)
    }
    fn busy(&self) -> bool {
        self.app.native.busy() || self.app.handoff.busy()
    }
    fn native(&self) -> bool {
        self.app.step == Step::Review && self.app.native_target()
    }
    fn command(&mut self, key: &str) {
        if self.busy() || self.app.native.page == Page::Finished || self.app.firmware_dialog {
            return;
        }
        let f = self.app.family();
        let bits: Vec<_> = key.split('/').collect();
        let index = |at: usize| bits.get(at).and_then(|v| v.parse::<usize>().ok());
        let action = match bits[0] {
            "family" => index(1)
                .and_then(|i| Family::ALL.get(i).copied())
                .filter(|f| crate::model::family_ready(*f))
                .and_then(|f| {
                    if self.app.still {
                        Some(Action::Family(f))
                    } else {
                        self.pending_family = Some((f, Instant::now()));
                        None
                    }
                }),
            "distro" => index(1)
                .filter(|i| *i < self.app.catalog.family(f).count())
                .map(|i| Action::Distro(f, i)),
            "edition" => index(1)
                .filter(|i| *i < self.app.distro().editions.len())
                .map(|i| Action::Edition(f, i)),
            "variant" => index(1)
                .filter(|i| *i < self.app.distro().variants.len())
                .map(|i| Action::Variant(f, i)),
            "mode" => Some(Action::Mode(
                f,
                if bits.get(1) == Some(&"compose") {
                    DesktopMode::Compose
                } else {
                    DesktopMode::Bare
                },
            )),
            "compose" => self
                .app
                .selection
                .compose_target(&self.app.catalog, f)
                .map(|target| Action::Compose(f, target)),
            "compositor" => bits.get(1).map(|id| Action::Compositor(f, (*id).into())),
            "shell" => bits.get(1).map(|id| {
                Action::Shell(
                    f,
                    if *id == "none" { ShellPick::NoShell } else { ShellPick::Shell((*id).into()) },
                )
            }),
            "extra" => bits.get(1).map(|id| {
                Action::Extra(f, (*id).into(), !self.app.selection.extra(&self.app.catalog, f, id))
            }),
            "network" => {
                self.network = index(1).filter(|i| *i < self.app.wifi.networks.len());
                self.network_name = self
                    .network
                    .and_then(|i| self.app.wifi.networks.get(i))
                    .map(|n| n.ssid.clone());
                None
            }
            "whole" | "alongside" => {
                let minimum = if self.native() { 40 } else { 32 };
                let disks =
                    if self.native() { &self.app.native.disks } else { &self.app.handoff.disks };
                let pick = index(1).and_then(|i| {
                    disks.get(i).filter(|d| d.usable()).and_then(|d| {
                        if bits[0] == "whole" {
                            Some(Where::Whole { disk: i })
                        } else {
                            index(2).and_then(|j| {
                                d.parts
                                    .get(j)
                                    .filter(|p| p.fs == "ntfs")
                                    .and_then(|p| p.max_linux_size)
                                    .filter(|m| *m >= minimum * handoff::GIB)
                                    .map(|m| Where::Alongside {
                                        disk: i,
                                        part: j,
                                        size: (60 * handoff::GIB).min(m),
                                    })
                            })
                        }
                    })
                });
                if let Some(pick) = pick {
                    if self.native() {
                        self.app.native.invalidate();
                        self.app.native.pick = Some(pick);
                    } else {
                        self.app.apply(Action::HandoffChoose(pick));
                    }
                }
                None
            }
            "app" => {
                if self.native()
                    && self.app.native.page == Page::Account
                    && let Some(id) = bits.get(1)
                {
                    self.app.native.invalidate();
                    if !self.app.native.apps.remove(*id) {
                        self.app.native.apps.insert((*id).into());
                    }
                }
                None
            }
            "refresh" => {
                if self.native() {
                    self.app.native.refresh_disks();
                } else {
                    self.app.handoff.retry();
                }
                None
            }
            _ => None,
        };
        if let Some(action) = action {
            self.app.apply(action);
        }
    }
    fn next(&mut self) {
        if self.pending_family.is_some() {
            return;
        }
        if self.app.firmware_dialog {
            return;
        }
        if self.native() {
            let n = &mut self.app.native;
            match n.page {
                Page::Storage if n.pick.is_some() => n.page = Page::Account,
                Page::Account => n.review(),
                Page::Review => n.start(),
                Page::Running if n.stopped() => n.reboot(),
                Page::Running if n.error.is_some() => n.retry_progress(),
                Page::Finished => n.reboot(),
                _ => {}
            }
        } else if self.app.step == Step::Review && self.app.handoff_target().is_some() {
            match self.app.handoff.phase {
                Phase::Choosing => self.app.handoff.review(),
                Phase::Reviewed(_) => self.app.apply(Action::Next),
                Phase::Failed(_) => self.app.handoff.retry(),
                _ => {}
            }
        } else {
            self.app.apply(Action::Next);
        }
    }
    fn back(&mut self) {
        self.pending_family = None;
        if self.busy() || self.app.native.page == Page::Finished || self.app.firmware_dialog {
            return;
        }
        if self.native() && self.app.native.page != Page::Storage {
            let n = &mut self.app.native;
            let prior = n.page;
            n.invalidate();
            n.page = if prior == Page::Review { Page::Account } else { Page::Storage };
        } else {
            self.app.apply(Action::Back);
        }
    }
    fn region(&mut self, kind: &str, index: i32) {
        if !self.native() || self.app.native.page != Page::Account {
            return;
        }
        let Ok(i) = usize::try_from(index) else { return };
        let n = &mut self.app.native;
        n.invalidate();
        match kind {
            "language" => {
                if let Some(v) = borea_locale::LOCALES.get(i) {
                    n.locale.lang = v.id.into();
                    n.touch("language");
                }
            }
            "keyboard" => {
                if let Some(v) = borea_locale::KEYBOARDS.get(i) {
                    n.locale.keyboard.layout = v.layout.into();
                    n.locale.keyboard.variant = v.variant.into();
                    n.touch("keyboard");
                    n.follow_keyboard();
                }
            }
            "console" => {
                if let Some(v) = borea_locale::console_keymaps().nth(i) {
                    n.locale.keyboard.console = v.into();
                    n.touch("console");
                }
            }
            "timezone" => {
                if let Some(v) = borea_locale::TIMEZONES.get(i) {
                    n.locale.timezone = (*v).into();
                    n.touch("timezone");
                }
            }
            _ => {}
        }
    }
    fn space(&mut self, gib: f32) {
        if !gib.is_finite() || self.busy() {
            return;
        }
        let native = self.native();
        let (disks, pick) = if native {
            (&self.app.native.disks, &self.app.native.pick)
        } else {
            (&self.app.handoff.disks, &self.app.handoff.pick)
        };
        if let Some(Where::Alongside { disk, part, .. }) = pick
            && let Some(max) =
                disks.get(*disk).and_then(|d| d.parts.get(*part)).and_then(|p| p.max_linux_size)
        {
            let pick = Where::Alongside {
                disk: *disk,
                part: *part,
                size: ((gib.round().max(if native { 40. } else { 32. }) as u64) * handoff::GIB)
                    .min(max),
            };
            if native {
                self.app.native.invalidate();
                self.app.native.pick = Some(pick);
            } else {
                self.app.apply(Action::HandoffChoose(pick));
            }
        }
    }
    fn tiles(&self) -> (Vec<Tile>, Vec<Section>) {
        let a = &self.app;
        let f = a.family();
        let c = a.selection.choice(f);
        let mut heroes = vec![];
        let mut groups = vec![];
        match a.step {
            Step::Connect => groups.push(section(
                "Available networks",
                a.wifi
                    .networks
                    .iter()
                    .enumerate()
                    .map(|(i, n)| {
                        Tile::new(
                            format!("network/{i}"),
                            &n.ssid,
                            format!(
                                "{}% · {}{}",
                                n.strength,
                                n.security,
                                if n.active { " · Connected" } else { "" }
                            ),
                            self.network == Some(i),
                        )
                    })
                    .collect(),
            )),
            Step::Machine => {
                if let Some(hw) = a.hardware() {
                    groups.push(section(
                        "Your machine",
                        hardware_facts(hw)
                            .into_iter()
                            .map(|(name, value, detail)| {
                                let mut t =
                                    Tile::new("", name, format!("{value}\n{detail}"), false);
                                t.enabled = false;
                                t.info = true;
                                t
                            })
                            .collect(),
                    ));
                }
            }
            Step::System => {
                for (i, f) in Family::ALL.into_iter().enumerate() {
                    let mut t = Tile::new(
                        format!("family/{i}"),
                        f.to_string(),
                        a.catalog
                            .family(f)
                            .map(|d| d.name.as_str())
                            .collect::<Vec<_>>()
                            .join(" · "),
                        self.picked() == Some(f),
                    );
                    t.logo = family_id(f).into();
                    t.enabled = crate::model::family_ready(f);
                    t.meta = match f {
                        Family::Arch => "Rolling release",
                        Family::Fedora => "Regular releases",
                        Family::Debian => "Stable release",
                        Family::Opensuse => "Rolling snapshots",
                    }
                    .into();
                    if !t.enabled {
                        t.meta = "Not available yet".into();
                        t.badge = Some("Coming soon".into());
                    }
                    heroes.push(t);
                }
            }
            Step::Distribution => {
                for (i, d) in a.catalog.family(f).enumerate() {
                    let status = if d.id.as_str() == "omarchy" {
                        "Official installer".into()
                    } else if matches!(d.id.as_str(), "arch" | "cachyos" | "tumbleweed") {
                        "Noctalia v5".into()
                    } else {
                        format!("{} integration", crate::model::capitalize(&d.status.to_string()))
                    };
                    let mut t = Tile::new(
                        format!("distro/{i}"),
                        &d.name,
                        &d.summary,
                        a.picked_distro && c.distro == i,
                    );
                    t.badge = Some(status);
                    t.logo = d.id.to_string();
                    // The family's flagship says what its family card says.
                    t.meta = if i == 0 {
                        crate::model::family_release(f).into()
                    } else {
                        crate::model::release_label(d.channel).into()
                    };
                    heroes.push(t);
                }
                if a.picked_distro {
                    groups.push(section(
                        "Edition",
                        a.distro()
                            .editions
                            .iter()
                            .enumerate()
                            .map(|(i, e)| {
                                Tile::new(
                                    format!("edition/{i}"),
                                    &e.name,
                                    if e.composable {
                                        "No desktop · compose your own"
                                    } else {
                                        "Desktop included"
                                    },
                                    a.picked_edition && c.edition == i,
                                )
                            })
                            .collect(),
                    ));
                    if a.picked_edition && !a.distro().variants.is_empty() {
                        groups.push(section(
                            "Image variant",
                            a.distro()
                                .variants
                                .iter()
                                .enumerate()
                                .map(|(i, v)| {
                                    Tile::new(
                                        format!("variant/{i}"),
                                        v,
                                        "Upstream image",
                                        a.picked_variant && c.variant == i,
                                    )
                                })
                                .collect(),
                        ));
                    }
                }
            }
            Step::Desktop => {
                if a.edition().composable {
                    groups.push(section(
                        "Desktop",
                        vec![
                            Tile::new(
                                "mode/compose",
                                "Compose a desktop",
                                "Choose a compositor and shell",
                                a.picked_mode && c.mode == DesktopMode::Compose,
                            ),
                            Tile::new(
                                "mode/bare",
                                "No desktop",
                                "Console login only",
                                a.picked_mode && c.mode == DesktopMode::Bare,
                            ),
                        ],
                    ));
                    if a.picked_mode && c.mode == DesktopMode::Compose {
                        groups.push(section(
                            "Compositor",
                            a.catalog
                                .desktops
                                .compositors(&a.distro().id)
                                .map(|o| {
                                    let mut t = Tile::new(
                                        format!("compositor/{}", o.item.id),
                                        &o.item.name,
                                        &o.item.style,
                                        a.picked_compositor
                                            && c.compositor.as_deref() == Some(&o.item.id),
                                    );
                                    t.enabled = o.available();
                                    if !t.enabled {
                                        t.badge = Some("Not available".into());
                                    }
                                    t
                                })
                                .collect(),
                        ));
                        if a.picked_compositor {
                            let comp = c.compositor.as_deref().unwrap_or_default();
                            let mut shells: Vec<_> = a
                                .catalog
                                .desktops
                                .ranked(&a.distro().id, comp)
                                .map(|o| {
                                    let mut t = Tile::new(
                                        format!("shell/{}", o.item.id),
                                        &o.item.name,
                                        &o.item.summary,
                                        a.picked_shell
                                            && c.shell == Some(ShellPick::Shell(o.item.id.clone())),
                                    );
                                    t.enabled = o.available();
                                    if !t.enabled {
                                        t.badge = Some("Not available".into());
                                    }
                                    if o.item.id.starts_with("cachyos-noctalia-") {
                                        t.description.push_str("\nPreview: CachyOS preset integration is pending. Choose Noctalia v5 for the native installer.");
                                    }
                                    t
                                })
                                .collect();
                            shells.sort_by_key(|t| t.key != "shell/noctalia");
                            shells.push(Tile::new(
                                "shell/none",
                                "No shell",
                                "Only the selected compositor",
                                a.picked_shell && c.shell == Some(ShellPick::NoShell),
                            ));
                            groups.push(section("Shell", shells));
                        }
                    }
                } else {
                    groups.push(section(
                        "Included desktop",
                        vec![{
                            let mut t = Tile::new(
                                "",
                                &a.edition().name,
                                "The upstream desktop is included",
                                true,
                            );
                            t.badge = Some("Included".into());
                            t
                        }],
                    ));
                    if a.selection.compose_target(&a.catalog, f).is_some() {
                        groups.push(section(
                            "Make it yours",
                            vec![Tile::new(
                                "compose/own",
                                "Compose a desktop",
                                "Switch to a minimal edition",
                                false,
                            )],
                        ));
                    }
                }
            }
            Step::Review if self.native() && a.native.page == Page::Storage => {
                groups.extend(storage_tiles(&a.native.disks, &a.native.pick, 40))
            }
            Step::Review if self.native() => {}
            Step::Review if a.handoff_target().is_some() => {
                if matches!(
                    a.handoff.phase,
                    Phase::Choosing | Phase::Reviewing | Phase::Reviewed(_)
                ) {
                    groups.extend(storage_tiles(&a.handoff.disks, &a.handoff.pick, 32));
                    // Extras are chosen here, on the review, and nowhere else.
                    groups.push(section("Extras", extra_tiles(a, f)));
                }
            }
            _ => {}
        }
        (heroes, groups)
    }
}
/// The extras offered for this card, picked or not: chosen only on the review.
fn extra_tiles(a: &crate::live::LiveInstaller, f: Family) -> Vec<Tile> {
    a.catalog
        .extras
        .on(&a.distro().id)
        .map(|o| {
            let mut t = Tile::new(
                format!("extra/{}", o.extra.id),
                &o.extra.name,
                o.describe(),
                a.selection.extra(&a.catalog, f, &o.extra.id),
            );
            t.enabled = !o.provision.built_in;
            if !t.enabled {
                t.badge = Some("Included".into());
            }
            t
        })
        .collect()
}
fn family_id(f: Family) -> &'static str {
    match f {
        Family::Arch => "arch",
        Family::Fedora => "fedora",
        Family::Debian => "debian",
        Family::Opensuse => "opensuse",
    }
}
fn storage_tiles(disks: &[handoff::Disk], pick: &Option<Where>, minimum: u64) -> Vec<Section> {
    let mut choices = vec![];
    for (i, d) in
        disks.iter().enumerate().filter(|(_, d)| d.usable() && d.size() >= minimum * handoff::GIB)
    {
        choices.push(Tile::new(
            format!("whole/{i}"),
            format!("Erase {}", d.name()),
            format!(
                "{} · {} · All data on this disk will be erased",
                d.path(),
                handoff::gb(d.size())
            ),
            matches!(pick,Some(Where::Whole{disk})if *disk==i),
        ));
        for (j, p) in d.parts.iter().enumerate().filter(|(_, p)| {
            p.fs == "ntfs" && p.max_linux_size.is_some_and(|m| m >= minimum * handoff::GIB)
        }) {
            choices.push(Tile::new(
                format!("alongside/{i}/{j}"),
                format!("Alongside Windows on {}", d.name()),
                format!("Shrink {} and keep Windows", p.path),
                matches!(pick,Some(Where::Alongside{disk,part,..})if *disk==i&&*part==j),
            ));
        }
    }
    choices.push(Tile::new("refresh/disks", "Refresh drives", "Read the drive list again", false));
    vec![section("Storage", choices)]
}

#[derive(Clone)]
enum Message {
    Tick(Instant),
    Key(iced::keyboard::Event),
    Choose(String),
    Next,
    Back,
    Action(Action),
    Field(&'static str, String),
    Region(&'static str, String),
    Space(f32),
    Confirm(bool),
    Apps(bool),
    /// The account password is also the sudo password.
    SameSudo(bool),
    Encrypt(bool),
    ShowPassword,
    /// Open or close an Extra apps category, by its label.
    Category(&'static str),
    /// Choose the disk unlock keyboard instead of following the keyboard.
    ConsoleChange,
    WifiPassword(String),
    Reveal(bool),
    Join,
    Rescan,
    Hidden(bool),
    HiddenSsid(String),
    HiddenSecurity(String),
    /// The pointer entered (true) or left (false) the card with this key.
    Hover(String, bool),
    /// Where the pointer is over the hovered card, from its top-left corner.
    Pointer(iced::Point),
}
impl std::fmt::Debug for Message {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Iced requires Debug, but credentials and network names never belong in diagnostics.
        f.write_str(match self {
            Self::Field(..) | Self::WifiPassword(..) | Self::HiddenSsid(..) => "Input([redacted])",
            Self::Tick(..) => "Tick",
            Self::Key(..) => "Key",
            Self::Choose(..) => "Choose",
            Self::Next => "Next",
            Self::Back => "Back",
            Self::Action(..) => "Action",
            Self::Region(..) => "Region",
            Self::Space(..) => "Space",
            Self::Confirm(..) => "Confirm",
            Self::Apps(..) => "Apps",
            Self::SameSudo(..) => "SameSudo",
            Self::Encrypt(..) => "Encrypt",
            Self::ShowPassword => "ShowPassword",
            Self::Category(..) => "Category",
            Self::ConsoleChange => "ConsoleChange",
            Self::Reveal(..) => "Reveal",
            Self::Join => "Join",
            Self::Rescan => "Rescan",
            Self::Hidden(..) => "Hidden",
            Self::HiddenSecurity(..) => "HiddenSecurity",
            Self::Hover(..) => "Hover",
            Self::Pointer(..) => "Pointer",
        })
    }
}
const FONT: &[u8] = include_bytes!("../assets/NotoSans-Regular.ttf");
const FACE: iced::Font = iced::Font::with_name("Noto Sans");
/// Card titles: Noto Sans SemiBold.
const TITLE_FONT: &[u8] = include_bytes!("../assets/NotoSans-SemiBold.ttf");
const TITLE: iced::Font = iced::Font { weight: iced::font::Weight::Semibold, ..FACE };
/// Each family's own type, where it publishes a free one (all SIL OFL 1.1): Fedora's
/// wordmark face and body text, and openSUSE's title face.
const BRAND_FONTS: [&[u8]; 3] = [
    include_bytes!("../assets/Comfortaa-Bold.ttf"),
    include_bytes!("../assets/OpenSans-Regular.ttf"),
    include_bytes!("../assets/SourceSans3-Semibold.ttf"),
];
/// The family's name on its card. Arch publishes no brand typeface, and Debian's
/// wordmark face is proprietary, so both keep Borea's own.
fn label_font(family: Family) -> iced::Font {
    use iced::font::{Family as Face, Weight};
    match family {
        Family::Fedora => {
            iced::Font { family: Face::Name("Comfortaa"), weight: Weight::Bold, ..FACE }
        }
        Family::Opensuse => {
            iced::Font { family: Face::Name("Source Sans 3"), weight: Weight::Semibold, ..FACE }
        }
        Family::Arch | Family::Debian => FACE,
    }
}
/// The family's quote on its card.
fn quote_font(family: Family) -> iced::Font {
    match family {
        Family::Fedora | Family::Opensuse => iced::Font::with_name("Open Sans"),
        Family::Arch | Family::Debian => FACE,
    }
}
struct App {
    bridge: Bridge,
    wind: crate::frost_motion::FrostMotion,
    last: Instant,
    view_key: String,
    elapsed: f32,
    pulse: f32,
    /// The card under the pointer, if it can be picked, and where on it.
    hovered: Option<String>,
    pointer: Option<iced::Point>,
    /// Per card key, eased 0 to 1: how far each card has lifted under the pointer,
    /// and how far its picked wash has come in. Settled cards drop out.
    lift: BTreeMap<String, f32>,
    chosen: BTreeMap<String, f32>,
    /// The card just picked, which alone blooms while `pulse` fades.
    bloom: Option<String>,
    password: zeroize::Zeroizing<String>,
    confirmation: zeroize::Zeroizing<String>,
    admin: zeroize::Zeroizing<String>,
    admin_confirmation: zeroize::Zeroizing<String>,
    show_password: bool,
    /// The field last typed in or chosen: its line turns ice, and its card takes
    /// the ice rim. Iced reports no focus, so this stands in for it.
    focus: Option<&'static str>,
    /// Open Extra apps categories, by label.
    categories: BTreeSet<&'static str>,
    wifi_password: zeroize::Zeroizing<String>,
    reveal: bool,
    hidden: bool,
    hidden_ssid: String,
    hidden_security: String,
    apps_open: bool,
    boot_smoke: bool,
    ready: bool,
}
impl App {
    fn new(live: bool, boot_smoke: bool) -> Self {
        let mut app = LiveInstaller::for_desktop();
        app.still = false;
        let _ = live; // Both binaries use the same reviewed workflow.
        Self::from_workflow(app, boot_smoke)
    }
    fn from_workflow(app: LiveInstaller, boot_smoke: bool) -> Self {
        Self {
            bridge: Bridge { app, network: None, network_name: None, pending_family: None },
            wind: Default::default(),
            last: Instant::now(),
            view_key: String::new(),
            elapsed: 0.,
            pulse: 1.,
            hovered: None,
            pointer: None,
            lift: BTreeMap::new(),
            chosen: BTreeMap::new(),
            bloom: None,
            password: Default::default(),
            confirmation: Default::default(),
            admin: Default::default(),
            admin_confirmation: Default::default(),
            show_password: false,
            focus: None,
            categories: BTreeSet::new(),
            wifi_password: Default::default(),
            reveal: false,
            hidden: false,
            hidden_ssid: String::new(),
            hidden_security: "WPA2 Personal".into(),
            apps_open: false,
            boot_smoke,
            ready: false,
        }
    }
    /// Ease every card's lift toward the pointer and its wash toward being picked,
    /// including the step rail's. Paused, both snap: the state still shows, without motion.
    fn ease_cards(&mut self, dt: f64, paused: bool) {
        let (heroes, groups) = self.bridge.tiles();
        let mut picked: BTreeSet<String> = heroes
            .iter()
            .chain(groups.iter().flat_map(|g| g.choices.iter()))
            .filter(|t| t.selected && !t.key.is_empty())
            .map(|t| t.key.clone())
            .collect();
        picked.insert(step_key(self.bridge.app.step));
        let hovered = self.hovered.clone();
        let settle = |value: &mut f32, target: f32, time: f64| {
            *value = if paused {
                target
            } else {
                *value + (target - *value) * (1. - (-dt / time).exp()) as f32
            };
        };
        for key in hovered.iter() {
            self.lift.entry(key.clone()).or_insert(0.);
        }
        for (key, value) in self.lift.iter_mut() {
            settle(value, if hovered.as_ref() == Some(key) { 1. } else { 0. }, 0.09);
        }
        self.lift.retain(|key, value| *value > 0.001 || hovered.as_ref() == Some(key));
        for key in &picked {
            self.chosen.entry(key.clone()).or_insert(0.);
        }
        for (key, value) in self.chosen.iter_mut() {
            settle(value, if picked.contains(key) { 1. } else { 0. }, 0.1);
        }
        self.chosen.retain(|key, value| *value > 0.001 || picked.contains(key));
    }
    fn key(&self) -> String {
        format!(
            "{:?}:{:?}:{:?}",
            self.bridge.app.step,
            self.bridge.app.native.page,
            self.bridge.pending_family.map(|(f, _)| f)
        )
    }
    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Key(iced::keyboard::Event::KeyPressed { key, modifiers, .. }) => {
                use iced::keyboard::{Key, key::Named};
                match key.as_ref() {
                    Key::Named(Named::Tab) => {
                        return if modifiers.shift() {
                            iced::widget::operation::focus_previous()
                        } else {
                            iced::widget::operation::focus_next()
                        };
                    }
                    Key::Named(Named::Escape) if self.bridge.app.firmware_dialog => {
                        self.bridge.app.apply(Action::CancelFirmware)
                    }
                    Key::Named(Named::ArrowLeft) if modifiers.control() => self.bridge.back(),
                    Key::Named(Named::ArrowRight)
                        if modifiers.control()
                            && self.bridge.app.step != Step::Review
                            && self.nav().1 =>
                    {
                        self.bridge.next()
                    }
                    Key::Character(s) if !modifiers.control() && !self.bridge.busy() => {
                        if let Ok(index) = s.parse::<usize>() {
                            let (heroes, groups) = self.bridge.tiles();
                            let choices = if heroes.is_empty() {
                                groups.first().map(|g| g.choices.clone()).unwrap_or_default()
                            } else {
                                heroes
                            };
                            if let Some(tile) = index
                                .checked_sub(1)
                                .and_then(|i| choices.get(i))
                                .filter(|t| t.enabled)
                            {
                                self.bridge.command(&tile.key);
                                self.pulse = 1.;
                                self.bloom = Some(tile.key.clone());
                            }
                        }
                    }
                    _ => {}
                }
            }
            Message::Tick(now) => {
                let dt = now.saturating_duration_since(self.last).as_secs_f64().min(0.1);
                self.last = now;
                self.bridge.app.poll();
                if let Some(name) = &self.bridge.network_name {
                    self.bridge.network =
                        self.bridge.app.wifi.networks.iter().position(|n| n.ssid == *name);
                    if self.bridge.network.is_none() && !self.hidden {
                        use zeroize::Zeroize;
                        self.wifi_password.zeroize();
                    }
                } else if !self.bridge.app.wifi.connected()
                    && let Some(name) = self.bridge.app.suggested_network()
                {
                    self.bridge.network =
                        self.bridge.app.wifi.networks.iter().position(|n| n.ssid == name);
                    self.bridge.network_name = self
                        .bridge
                        .network
                        .and_then(|i| self.bridge.app.wifi.networks.get(i))
                        .map(|n| n.ssid.clone());
                }
                if self.bridge.app.step != Step::System {
                    self.bridge.pending_family = None;
                }
                if let Some((f, at)) = self.bridge.pending_family
                    && (self.bridge.app.still
                        || now.saturating_duration_since(at) >= Duration::from_millis(280))
                {
                    self.bridge.pending_family = None;
                    self.bridge.app.apply(Action::Family(f));
                }
                let key = self.key();
                let changed = key != self.view_key;
                self.view_key = key;
                let paused = self.bridge.app.still;
                self.wind.advance(dt, changed, paused, self.bridge.app.install_progress());
                if !paused {
                    self.elapsed += dt as f32;
                    self.pulse = (self.pulse - dt as f32 / 0.7).max(0.);
                }
                if changed {
                    // A card that vanishes under the pointer never reports it leaving.
                    self.hovered = None;
                    self.pointer = None;
                }
                self.ease_cards(dt, paused);
                if self.bridge.app.native.page != Page::Account {
                    use zeroize::Zeroize;
                    self.password.zeroize();
                    self.confirmation.zeroize();
                    self.admin.zeroize();
                    self.admin_confirmation.zeroize();
                }
                if !self.bridge.app.encryption_offered() && self.bridge.app.native.encrypt {
                    self.bridge.app.native.encrypt = false;
                }
                if self.boot_smoke && !self.ready {
                    self.ready = true;
                    println!("BOREA_GUI_READY");
                    return iced::exit();
                }
            }
            Message::Action(action) => {
                if !self.bridge.app.firmware_dialog
                    || matches!(action, Action::CancelFirmware | Action::RestartFirmware)
                {
                    self.bridge.app.apply(action);
                }
            }
            Message::Hover(key, inside) => {
                if inside {
                    self.hovered = Some(key);
                } else if self.hovered.as_ref() == Some(&key) {
                    self.hovered = None;
                    self.pointer = None;
                }
            }
            Message::Pointer(at) => self.pointer = Some(at),
            Message::Back => self.bridge.back(),
            Message::Next => self.bridge.next(),
            Message::Choose(key) => {
                let prior = self.bridge.network;
                self.bloom = Some(key.clone());
                self.bridge.command(&key);
                if prior != self.bridge.network {
                    use zeroize::Zeroize;
                    self.wifi_password.zeroize();
                }
                self.pulse = 1.;
            }
            Message::Field(field, value)
                if self.bridge.native()
                    && !self.bridge.busy()
                    && !self.bridge.app.firmware_dialog =>
            {
                match field {
                    "password" => *self.password = value.clone(),
                    "confirmation" => *self.confirmation = value.clone(),
                    "admin" => *self.admin = value.clone(),
                    "admin-confirmation" => *self.admin_confirmation = value.clone(),
                    _ => {}
                }
                self.focus = Some(field);
                self.bridge.app.native.edit_account(field, value);
            }
            Message::SameSudo(same)
                if self.bridge.native()
                    && !self.bridge.busy()
                    && !self.bridge.app.firmware_dialog =>
            {
                self.bridge.app.native.same_sudo(same);
                if same {
                    use zeroize::Zeroize;
                    self.admin.zeroize();
                    self.admin_confirmation.zeroize();
                }
            }
            Message::Encrypt(on)
                if self.bridge.native()
                    && !self.bridge.app.firmware_dialog
                    && self.bridge.app.encryption_offered() =>
            {
                self.bridge.app.native.set_encrypt(on)
            }
            Message::ShowPassword => self.show_password = !self.show_password,
            Message::Category(label) => {
                if !self.categories.remove(label) {
                    self.categories.insert(label);
                }
            }
            Message::ConsoleChange
                if self.bridge.native() && self.bridge.app.native.page == Page::Account =>
            {
                self.bridge.app.native.touch("console");
                self.focus = Some("console");
            }
            Message::Region(kind, value)
                if !self.bridge.busy() && !self.bridge.app.firmware_dialog =>
            {
                let index = region_options(kind).iter().position(|s| s == &value);
                self.focus = Some(kind);
                if let Some(i) = index {
                    self.bridge.region(kind, i as i32);
                }
            }
            Message::Space(v) if !self.bridge.app.firmware_dialog => self.bridge.space(v),
            Message::Confirm(on) if !self.bridge.busy() && !self.bridge.app.firmware_dialog => {
                if self.bridge.native() {
                    self.bridge.app.native.confirmed = on;
                } else {
                    self.bridge.app.handoff.confirm(on);
                }
            }
            Message::Apps(on) => self.apps_open = on,
            Message::WifiPassword(value) => *self.wifi_password = value,
            Message::Reveal(on) => self.reveal = on,
            Message::Hidden(on) => self.hidden = on,
            Message::HiddenSsid(value) => self.hidden_ssid = value,
            Message::HiddenSecurity(value) => self.hidden_security = value,
            Message::Rescan => self.bridge.app.wifi.send(crate::wifi::Command::Rescan),
            Message::Join
                if !self.bridge.busy() && !self.bridge.app.firmware_dialog && self.join_valid() =>
            {
                let security = self.network_security();
                let passphrase = (!matches!(
                    security,
                    borea_network::Security::Open | borea_network::Security::EnhancedOpen
                ))
                .then(|| borea_network::Secret::new(self.wifi_password.to_string()));
                let command = if self.hidden {
                    Some(crate::wifi::Command::JoinHidden {
                        ssid: self.hidden_ssid.clone(),
                        security,
                        passphrase,
                    })
                } else {
                    self.bridge
                        .network
                        .and_then(|i| self.bridge.app.wifi.networks.get(i))
                        .cloned()
                        .map(|network| crate::wifi::Command::Join { network, passphrase })
                };
                if let Some(command) = command {
                    self.bridge.app.wifi.send(command);
                }
                use zeroize::Zeroize;
                self.wifi_password.zeroize();
            }
            _ => {}
        }
        Task::none()
    }
    fn theme(&self) -> Theme {
        let p = self.colors();
        Theme::custom(
            "Borea frost",
            iced::theme::Palette {
                background: p.bg,
                text: p.text,
                primary: p.ice,
                success: Color::from_rgb8(163, 190, 140),
                warning: Color::from_rgb8(235, 203, 139),
                danger: Color::from_rgb8(191, 97, 106),
            },
        )
    }
    fn colors(&self) -> Colors {
        Colors::new(self.bridge.app.dark)
    }
    fn subscription(&self) -> Subscription<Message> {
        Subscription::batch([
            iced::time::every(tick(self.bridge.app.still, software_graphics())).map(Message::Tick),
            iced::keyboard::listen().map(Message::Key),
        ])
    }
    fn network_security(&self) -> borea_network::Security {
        if self.hidden {
            match self.hidden_security.as_str() {
                "Open" => borea_network::Security::Open,
                "WPA3 Personal" => borea_network::Security::Wpa3Personal,
                _ => borea_network::Security::WpaPersonal,
            }
        } else {
            self.bridge
                .network
                .and_then(|i| self.bridge.app.wifi.networks.get(i))
                .map_or(borea_network::Security::Open, |n| n.security)
        }
    }
    fn join_valid(&self) -> bool {
        let selected = if self.hidden {
            !self.hidden_ssid.trim().is_empty() && self.hidden_ssid.len() <= 32
        } else {
            self.bridge.network.is_some()
        };
        selected && crate::wifi::valid_passphrase(self.network_security(), &self.wifi_password)
    }
    fn nav(&self) -> (String, bool) {
        let b = &self.bridge;
        if b.native() {
            let n = &b.app.native;
            let label = match n.page {
                Page::Storage => "Account and region",
                Page::Account => "Check installation plan",
                Page::Review => "Install",
                Page::Running if n.stopped() => "Restart to recover",
                Page::Running => "Retry / reconnect",
                Page::Finished => "Restart",
            };
            let enabled = match n.page {
                Page::Storage => n.pick.is_some(),
                Page::Account => n.account_complete(),
                Page::Review => n.ready(),
                Page::Running => n.error.is_some(),
                _ => true,
            };
            return (label.into(), enabled && !n.loading());
        }
        if b.app.step == Step::Review && b.app.handoff_target().is_some() {
            return match &b.app.handoff.phase {
                Phase::Choosing => ("Check installation plan".into(), b.app.handoff.pick.is_some()),
                Phase::Reviewed(_) => ("Begin installation".into(), b.app.handoff.ready()),
                Phase::Failed(_) => ("Try again".into(), true),
                _ => ("Preparing…".into(), false),
            };
        }
        (
            if b.app.step == Step::Desktop { "Review" } else { "Continue" }.into(),
            b.app.can_continue() && b.pending_family.is_none(),
        )
    }
    fn heading(&self) -> String {
        if self.bridge.native() {
            match self.bridge.app.native.page {
                Page::Storage => format!("Where should {} go?", self.bridge.app.distro().name),
                Page::Account => "Make it yours.".into(),
                Page::Review => "One final check.".into(),
                Page::Running => "A new system is taking shape.".into(),
                Page::Finished => "Ready for a fresh start.".into(),
            }
        } else {
            self.bridge.app.step.title().into()
        }
    }
    fn view(&self) -> Element<'_, Message> {
        let p = self.colors();
        let a = &self.bridge.app;
        let mut rail = column![
            text("Borea").size(32).color(p.ice),
            text("Follow the wind.").size(13).color(p.muted),
            text("Arch, CachyOS, Tumbleweed and Omarchy installs work.").size(11).color(p.muted),
            space().height(28)
        ]
        .spacing(9);
        for step in Step::ALL {
            let active = step == a.step;
            let mut content = column![
                text(format!("{:02}  {}", step.number(), step.name()))
                    .size(14)
                    .color(if step <= a.reached { p.text } else { p.muted })
            ]
            .spacing(5);
            if let Some(summary) = a.summary(step) {
                content = content.push(text(summary).size(11).color(p.muted));
            }
            let enabled =
                step <= a.reached && !self.bridge.busy() && a.native.page != Page::Finished;
            rail = rail.push(self.card_shell(
                &step_key(step),
                content.into(),
                p.ice,
                Size::Rail,
                Look::Ready,
                active,
                enabled.then_some(Message::Action(Action::Go(step))),
            ));
        }
        rail = rail
            .push(space().height(Length::Fill))
            .push(text(a.wifi.connection_label()).size(12).color(p.muted))
            .push(
                button(if a.dark { "Light theme" } else { "Dark theme" })
                    .on_press(Message::Action(Action::ToggleTheme))
                    .style(button::text),
            )
            .push(
                button(if a.still { "Resume wind" } else { "Pause wind" })
                    .on_press(Message::Action(Action::ToggleMotion))
                    .style(button::text),
            );
        let firmware = button(
            row![
                svg(svg::Handle::from_memory(include_bytes!("../ui/cpu.svg").as_slice()))
                    .width(18)
                    .height(18)
                    .style(move |_, _| svg::Style { color: Some(p.ice) }),
                text("UEFI setup").size(12)
            ]
            .spacing(8),
        )
        .on_press_maybe(
            (a.firmware.control(a.uefi()).0 && !self.bridge.busy())
                .then_some(Message::Action(Action::OpenFirmware)),
        )
        .style(button::text);
        rail = rail.push(firmware);
        let rail = container(rail).width(212).height(Length::Fill).padding(20).style(move |_| {
            container::Style { background: Some(p.surface.into()), ..Default::default() }
        });
        let (heroes, groups) = self.bridge.tiles();
        let mut body = column![
            text(format!("Step {} of 6", a.step.number())).size(12).color(p.ice),
            text(self.heading()).size(31),
            text(a.step.lead()).size(13).color(p.muted)
        ]
        .spacing(12);
        if !heroes.is_empty() {
            // Hero cards sit two across, so each has room for its words; a last card
            // on its own keeps the same width.
            let mut grid = column![].spacing(12);
            for chunk in heroes.chunks(2) {
                let mut cards = row![].spacing(12);
                for tile in chunk {
                    cards = cards.push(self.card(tile.clone(), true));
                }
                if chunk.len() < 2 {
                    cards = cards.push(space().width(Length::Fill));
                }
                grid = grid.push(cards);
            }
            body = body.push(grid);
        }
        if self.bridge.native() && a.native.page == Page::Account {
            body = body.push(self.account());
        }
        if a.step == Step::Connect {
            body =
                body.push(button("Rescan networks").on_press(Message::Rescan).style(button::text));
        }
        for group in groups {
            body = body.push(text(group.title).size(12).color(p.ice));
            // Compact rows keep long shell/app catalogs readable at laptop resolutions.
            let across = if a.step == Step::Connect || a.step == Step::Review { 1 } else { 2 };
            for chunk in group.choices.chunks(across) {
                let mut choices = row![].spacing(10);
                for tile in chunk {
                    choices = choices.push(self.card(tile.clone(), false));
                }
                if chunk.len() < across {
                    choices = choices.push(space().width(Length::Fill));
                }
                body = body.push(choices);
            }
        }
        if a.step == Step::Connect {
            body = body.push(self.wifi());
        }
        if a.step == Step::Machine
            && let Some(hw) = a.hardware()
        {
            for finding in borea_distro::machine_findings(hw) {
                let (borea_distro::Finding::Recommend(note)
                | borea_distro::Finding::Unknown(note)
                | borea_distro::Finding::Blocker(note)) = finding;
                body = body.push(text(note).size(13).color(p.muted));
            }
        }
        if a.step == Step::Review {
            body = self.review(body);
        }
        let (label, enabled) = self.nav();
        let footer = row![
            button("Back")
                .on_press_maybe(
                    (a.step != Step::Connect
                        && !self.bridge.busy()
                        && a.native.page != Page::Finished)
                        .then_some(Message::Back)
                )
                .style(button::text),
            text(a.selection_title()).size(11).color(p.muted),
            space().width(Length::Fill),
            button(text(label).size(14))
                .padding([12, 24])
                .on_press_maybe(enabled.then_some(Message::Next))
        ]
        .spacing(12)
        .align_y(Alignment::Center);
        let main = column![
            scrollable(body.padding([8, 4])).id("installer-body").spacing(10).height(Length::Fill),
            footer
        ]
        .spacing(18);
        let content = container(
            row![rail, container(main).padding(28).width(Length::Fill)].height(Length::Fill),
        )
        .style(move |_| container::Style { text_color: Some(p.text), ..Default::default() });
        let background = widget::canvas(AirCanvas { trails: self.wind.trails(), dark: a.dark })
            .width(Length::Fill)
            .height(Length::Fill);
        let base: Element<'_, Message> = stack![background, content].into();
        if a.firmware_dialog {
            let dialog = column![text("Restart into UEFI setup?").size(24),
                text(if a.native.page == Page::Finished {
                    "Restart into firmware settings, then boot your installed system."
                } else {
                    "This restarts the computer. Your installer choices will be lost. Return to the Borea USB afterwards."
                }).size(14),
                text(a.firmware.error.clone().unwrap_or_default()).size(13),
                row![button("Cancel").on_press_maybe((!a.firmware.restarting).then_some(Message::Action(Action::CancelFirmware))),
                    button("Restart into firmware").on_press_maybe((!a.firmware.restarting).then_some(Message::Action(Action::RestartFirmware)))].spacing(12)].spacing(20);
            let modal = container(container(dialog).padding(28).max_width(520).style(move |_| {
                container::Style {
                    background: Some(p.surface.into()),
                    border: Border { color: p.ice, width: 1., radius: 16.into() },
                    ..Default::default()
                }
            }))
            .center(Length::Fill)
            .style(|_| container::Style {
                background: Some(Color::from_rgba(0., 0., 0., 0.72).into()),
                ..Default::default()
            });
            stack![base, iced::widget::opaque(modal)].into()
        } else {
            base
        }
    }
    /// The colours a card's words take: its name, its words, a credit or detail,
    /// and its release line. A card that can't be picked yet is muted throughout.
    fn ink(&self, waiting: bool) -> (Color, Color, Color, Color) {
        let p = self.colors();
        let soft = |c: Color, alpha: f32| Color { a: alpha, ..c };
        if waiting {
            (soft(p.muted, 0.6), soft(p.muted, 0.85), soft(p.muted, 0.6), soft(p.muted, 0.75))
        } else {
            (p.text, soft(p.text, 0.86), soft(p.muted, 0.75), mix(p.text, p.muted, 0.22))
        }
    }
    /// A hero card's bottom line: its release on the left and, at the bottom right,
    /// its quiet tag. Every hero card's release line sits level.
    fn foot<'a>(
        &self,
        release: String,
        badge: Option<String>,
        waiting: bool,
    ) -> Element<'a, Message> {
        let (_, _, _, ink) = self.ink(waiting);
        let mut foot = row![text(release).size(12.5).wrapping(text::Wrapping::None).color(ink)]
            .spacing(8)
            .align_y(Alignment::Center);
        foot = match badge {
            Some(label) => foot.push(space().width(Length::Fill)).push(tag(self.colors(), label)),
            None => foot.push(space().height(22)),
        };
        foot.into()
    }
    /// A quote, and who said it, beneath.
    fn quote<'a>(
        &self,
        quote: crate::model::Quote,
        font: iced::Font,
        waiting: bool,
    ) -> Element<'a, Message> {
        let (_, words, cite, _) = self.ink(waiting);
        column![
            text(format!("\u{201c}{}\u{201d}", quote.text))
                .size(14)
                .line_height(1.45)
                .wrapping(text::Wrapping::Word)
                .font(font)
                .color(words),
            text(format!("\u{2014} {}", quote.source)).size(11.5).font(font).color(cite),
        ]
        .spacing(6)
        .into()
    }
    /// A family's words: its logo, its name in its own type, its own words, and its release.
    fn family_body<'a>(&self, tile: &Tile, family: Family) -> Element<'a, Message> {
        let waiting = !tile.enabled;
        let (name, ..) = self.ink(waiting);
        let mut body = column![].spacing(14).height(Length::Fill);
        if let Some(mark) =
            faded_logo(&tile.logo, self.bridge.app.dark, if waiting { 0.38 } else { 1. })
        {
            body = body.push(mark);
        }
        body.push(text(tile.label.clone()).size(19).font(label_font(family)).color(name))
            .push(self.quote(crate::model::family_quote(family), quote_font(family), waiting))
            .push(space().height(Length::Fill))
            .push(self.foot(
                crate::model::family_release(family).into(),
                tile.badge.clone(),
                waiting,
            ))
            .into()
    }
    /// A distribution's words: as a family's, with its status tag at the bottom right.
    fn distro_body<'a>(&self, tile: &Tile) -> Element<'a, Message> {
        let waiting = !tile.enabled;
        let (name, words, ..) = self.ink(waiting);
        let mut body = column![].spacing(14).height(Length::Fill);
        if let Some(mark) =
            faded_logo(&tile.logo, self.bridge.app.dark, if waiting { 0.38 } else { 1. })
        {
            body = body.push(mark);
        }
        body = body.push(text(tile.label.clone()).size(19).color(name));
        body = match crate::model::distro_quote(&tile.logo) {
            Some(quote) => body.push(self.quote(quote, FACE, waiting)),
            None => {
                body.push(text(tile.description.clone()).size(14).line_height(1.45).color(words))
            }
        };
        body.push(space().height(Length::Fill))
            .push(self.foot(tile.meta.clone(), tile.badge.clone(), waiting))
            .into()
    }
    /// Any other choice's words: its name, its detail, and its tag at the bottom right.
    fn compact_body<'a>(&self, tile: &Tile) -> Element<'a, Message> {
        let waiting = !tile.enabled && !tile.info && !tile.selected;
        let (name, words, ..) = self.ink(waiting);
        let mut body = column![text(tile.label.clone()).size(15).color(name)].spacing(5);
        if !tile.description.is_empty() {
            body = body.push(text(tile.description.clone()).size(13).line_height(1.4).color(words));
        }
        if let Some(label) = &tile.badge {
            body = body.push(row![space().width(Length::Fill), tag(self.colors(), label.clone())]);
        }
        body.into()
    }
    /// One card in Borea's card language, whatever it holds. Hovering lifts it with
    /// a deeper shadow and a glow of its accent, and the side under the pointer gives
    /// way into shade. Picked, a wash of its accent fades in and drifts slowly, with
    /// a 2 px rim and a gently breathing glow; just picked, a faint bloom grows from
    /// the pointer. A card that can't be picked yet is greyed and still, and a card
    /// of facts is flat. Three layers: the surface, the shading drawn on it, and the
    /// words on top, so the shading never dims a logo or text.
    #[allow(clippy::too_many_arguments)]
    fn card_shell<'a>(
        &'a self,
        key: &str,
        words: Element<'a, Message>,
        accent: Color,
        size: Size,
        look: Look,
        picked: bool,
        press: Option<Message>,
    ) -> Element<'a, Message> {
        let p = self.colors();
        let a = &self.bridge.app;
        let still = a.still;
        let live = press.is_some() && !key.is_empty();
        let lift = if live { self.lift.get(key).copied().unwrap_or(0.) } else { 0. };
        // Paused, a pick shows at once; otherwise it eases in from the next tick.
        let settled = if picked && (still || key.is_empty()) { 1. } else { 0. };
        let chosen = self.chosen.get(key).copied().unwrap_or(settled);
        let t = self.elapsed;
        let angle =
            if still { 167.5 } else { 167.5 + 37.5 * (t * std::f32::consts::TAU / 18.).sin() };
        let breath =
            if still { 0.41 } else { 0.41 + 0.09 * (t * std::f32::consts::TAU / 6.8).sin() };
        let shade = if a.dark { Color::from_rgb8(8, 11, 18) } else { Color::from_rgb8(46, 52, 64) };
        let depth = if a.dark { (0.2, 0.42) } else { (0.08, 0.2) };
        // A form card holds fields to type in, so it is never a button itself.
        let words: Element<'a, Message> = if look == Look::Form {
            container(words).width(Length::Fill).into()
        } else {
            button(words)
                .padding(size.padding())
                .width(Length::Fill)
                .height(size.height())
                .on_press_maybe(press)
                .style(move |_, _| button::Style {
                    background: None,
                    text_color: p.text,
                    border: Border { radius: 16.into(), ..Default::default() },
                    ..Default::default()
                })
                .into()
        };
        let mut layers = stack![words];
        if live {
            layers = layers.push_under(
                widget::canvas(CardFx {
                    at: if self.hovered.as_deref() == Some(key) { self.pointer } else { None },
                    lift,
                    push: !still,
                    bloom: if picked && !still && self.bloom.as_deref() == Some(key) {
                        self.pulse
                    } else {
                        0.
                    },
                    brand: accent,
                    shade,
                })
                .width(Length::Fill)
                .height(Length::Fill),
            );
        }
        let layers = layers.push_under(
            container(space())
                .width(Length::Fill)
                .height(Length::Fill)
                .style(move |_| card_surface(p, accent, shade, depth, look, lift, chosen, angle)),
        );
        let card: Element<'a, Message> = if live {
            widget::mouse_area(layers)
                .on_enter(Message::Hover(key.to_string(), true))
                .on_move(Message::Pointer)
                .on_exit(Message::Hover(key.to_string(), false))
                .into()
        } else {
            layers.into()
        };
        // A form card's rim glows steadily while you type in it.
        let glow =
            if look == Look::Form { 0.3 * chosen } else { (0.36 * lift).max(breath * chosen) };
        let blur = size.glow();
        let card = container(card).style(move |_| container::Style {
            border: Border { radius: 16.into(), ..Default::default() },
            shadow: iced::Shadow {
                color: Color { a: glow, ..accent },
                offset: iced::Vector::new(0., 2. * size.rise() * lift * (1. - chosen)),
                blur_radius: blur,
            },
            ..Default::default()
        });
        let rise = size.rise();
        container(card)
            .padding(iced::Padding {
                top: rise * (1. - lift),
                bottom: rise * lift,
                ..Default::default()
            })
            .into()
    }
    /// Facts to read, on a card of facts: flat, like the machine's.
    fn facts<'a>(&'a self, words: String) -> Element<'a, Message> {
        let p = self.colors();
        let words = text(words).size(14).line_height(1.45).color(Color { a: 0.86, ..p.text });
        self.card_shell("", words.into(), p.ice, Size::Compact, Look::Info, false, None)
    }
    fn card(&self, tile: Tile, hero: bool) -> Element<'_, Message> {
        let p = self.colors();
        let a = &self.bridge.app;
        let press = (tile.enabled && !tile.key.is_empty() && !self.bridge.busy())
            .then(|| Message::Choose(tile.key.clone()));
        let look = if tile.info {
            Look::Info
        } else if !tile.enabled && !tile.selected {
            Look::Waiting
        } else {
            Look::Ready
        };
        let rgb = |[r, g, b]: [u8; 3]| Color::from_rgb8(r, g, b);
        if hero
            && let Some(i) = tile.key.strip_prefix("family/").and_then(|i| i.parse::<usize>().ok())
            && let Some(&family) = Family::ALL.get(i)
        {
            let accent = rgb(crate::model::family_colour(family));
            let words = self.family_body(&tile, family);
            return self.card_shell(
                &tile.key,
                words,
                accent,
                Size::Hero,
                look,
                tile.selected,
                press,
            );
        }
        if hero && tile.key.starts_with("distro/") {
            let accent =
                crate::model::distro_colour(&tile.logo, a.family()).map(rgb).unwrap_or(p.muted);
            let words = self.distro_body(&tile);
            return self.card_shell(
                &tile.key,
                words,
                accent,
                Size::Hero,
                look,
                tile.selected,
                press,
            );
        }
        let words = self.compact_body(&tile);
        self.card_shell(&tile.key, words, p.ice, Size::Compact, look, tile.selected, press)
    }
    /// The Account page: three titled form cards in the shared card language.
    /// Every field is a faint hint on a thin line; the only words added are errors,
    /// and whether the passwords match.
    fn account(&self) -> Element<'_, Message> {
        column![self.account_card(), self.region_card(), self.apps_card()].spacing(10).into()
    }
    /// A form card: the shared card shell, rimmed in ice while `active`.
    fn form_card<'a>(&'a self, words: Element<'a, Message>, active: bool) -> Element<'a, Message> {
        let p = self.colors();
        self.card_shell("", words, p.ice, Size::Form, Look::Form, active, None)
    }
    /// A card's title, at the top of its first section.
    fn form_title<'a>(&self, title: &'static str) -> Element<'a, Message> {
        let p = self.colors();
        container(text(title).size(15).font(TITLE).color(p.text))
            .padding(iced::Padding { top: 14., right: 16., bottom: 2., left: 16. })
            .into()
    }
    /// One field: a faint hint on a thin line that turns ice while you type in it.
    fn form_field<'a>(
        &self,
        key: &'static str,
        hint: &'static str,
        value: &str,
        secret: bool,
        size: f32,
        width: Length,
    ) -> Element<'a, Message> {
        let p = self.colors();
        column![
            text_input(hint, value)
                .secure(secret)
                .on_input(move |s| Message::Field(key, s))
                .size(size)
                .padding(iced::Padding { bottom: 6., left: 1., right: 1., ..Default::default() })
                .style(move |_, _| bare_input(p)),
            underline(p, self.focus == Some(key)),
        ]
        .width(width)
        .into()
    }
    fn account_card(&self) -> Element<'_, Message> {
        let a = &self.bridge.app;
        let n = &a.native;
        let p = self.colors();
        let initial: String = n
            .user
            .full_name
            .trim()
            .chars()
            .next()
            .into_iter()
            .flat_map(char::to_uppercase)
            .collect();
        let set = !initial.is_empty();
        let avatar = container(text(initial).size(22).color(p.text))
            .width(52)
            .height(52)
            .align_x(Alignment::Center)
            .align_y(Alignment::Center)
            .style(move |_| container::Style {
                background: set.then(|| {
                    Background::Gradient(iced::Gradient::Linear(
                        iced::gradient::Linear::new(iced::Degrees(160.))
                            .add_stop(0., mix(p.surface, p.ice, 0.45))
                            .add_stop(0.75, p.surface),
                    ))
                }),
                border: Border {
                    color: if set { p.line } else { Color { a: 0.45, ..p.muted } },
                    width: 1.,
                    radius: 26.into(),
                },
                ..Default::default()
            });
        // Username and hostname sit inline, each as wide as what's typed in it.
        let inline = |key: &'static str, hint: &'static str, value: &str| {
            let chars = if value.is_empty() { hint.len() } else { value.chars().count() };
            self.form_field(key, hint, value, false, 14., Length::Fixed(chars as f32 * 8.2 + 8.))
        };
        let handle = row![
            inline("username", "username", &n.user.username),
            text("@").size(14).color(p.muted),
            inline("hostname", "hostname", &n.user.hostname),
        ]
        .spacing(8);
        let who = column![
            self.form_field("full-name", "Your name", &n.user.full_name, false, 21., Length::Fill),
            handle
        ]
        .spacing(10);
        let mut you = column![row![avatar, who].spacing(14).align_y(Alignment::Center)];
        if let Some(problem) =
            crate::model::identity_problem(&n.user.full_name, &n.user.username, &n.user.hostname)
        {
            you = you.push(container(note(problem, p.warn)).padding(iced::Padding {
                top: 6.,
                left: 66.,
                ..Default::default()
            }));
        }
        // Password and confirm side by side, with the eye to show them.
        let shown = self.show_password;
        let eye = button(svg(svg::Handle::from_memory(EYE.as_bytes())).width(18).height(18).style(
            move |_, status| svg::Style {
                color: Some(if shown || matches!(status, svg::Status::Hovered) {
                    p.text
                } else {
                    p.muted
                }),
            },
        ))
        .width(30)
        .height(30)
        .padding(6)
        .on_press(Message::ShowPassword)
        .style(|_, _| button::Style::default());
        let password = row![
            self.form_field("password", "Password", &self.password, !shown, 15., Length::Fill),
            self.form_field(
                "confirmation",
                "Confirm password",
                &self.confirmation,
                !shown,
                15.,
                Length::Fill
            ),
            eye
        ]
        .spacing(16)
        .align_y(Alignment::End);
        // Any password goes: no strength rating, only whether the two match.
        let password = column![
            password,
            container(match_note(p, &self.password, &self.confirmation))
                .padding(iced::Padding { top: 6., ..Default::default() }),
        ];
        // Options: a sunk band under the last divider.
        let option = |label: &'static str, on: bool, toggle: fn(bool) -> Message| {
            container(
                row![
                    text(label).size(14).color(p.text),
                    space().width(Length::Fill),
                    toggler(on)
                        .on_toggle(toggle)
                        .size(20)
                        .style(move |_, status| switch(p, status))
                ]
                .align_y(Alignment::Center),
            )
            .padding([11, 16])
        };
        let mut options =
            column![option("Use as sudo password", n.user.same_passphrase, Message::SameSudo)];
        if !n.user.same_passphrase {
            options = options.push(
                container(column![
                    row![
                        self.form_field(
                            "admin",
                            "Sudo password",
                            &self.admin,
                            !shown,
                            15.,
                            Length::Fill
                        ),
                        self.form_field(
                            "admin-confirmation",
                            "Confirm sudo password",
                            &self.admin_confirmation,
                            !shown,
                            15.,
                            Length::Fill
                        ),
                    ]
                    .spacing(16),
                    container(match_note(p, &self.admin, &self.admin_confirmation))
                        .padding(iced::Padding { top: 6., ..Default::default() }),
                ])
                .padding(iced::Padding {
                    top: 2.,
                    right: 16.,
                    bottom: 12.,
                    left: 32.,
                }),
            );
        }
        if a.encryption_offered() {
            options = options.push(divider(p, 0.45)).push(option(
                "Encrypt disk",
                n.encrypt,
                Message::Encrypt,
            ));
        }
        let band = container(options).width(Length::Fill).style(move |_| container::Style {
            background: Some(mix(p.surface, p.bg, 0.4).into()),
            border: Border {
                radius: iced::border::Radius {
                    top_left: 0.,
                    top_right: 0.,
                    bottom_right: 15.,
                    bottom_left: 15.,
                },
                ..Default::default()
            },
            ..Default::default()
        });
        let mut words = column![
            self.form_title("Account"),
            container(you).padding(iced::Padding { top: 12., right: 16., bottom: 14., left: 16. }),
            divider(p, 0.65),
            container(password).padding(iced::Padding {
                top: 12.,
                right: 16.,
                bottom: 14.,
                left: 16.
            }),
        ];
        if let Some(e) = n.error.clone().filter(|_| n.page == Page::Account) {
            words = words.push(container(note(e, p.warn)).padding(iced::Padding {
                bottom: 12.,
                left: 16.,
                right: 16.,
                ..Default::default()
            }));
        }
        let words = words.push(divider(p, 0.65)).push(band);
        let active = matches!(
            self.focus,
            Some(
                "full-name"
                    | "username"
                    | "hostname"
                    | "password"
                    | "confirmation"
                    | "admin"
                    | "admin-confirmation"
            )
        );
        self.form_card(words.into(), active)
    }
    fn region_card(&self) -> Element<'_, Message> {
        let n = &self.bridge.app.native;
        let p = self.colors();
        let choose = |kind: &'static str, hint: &'static str| -> Element<'_, Message> {
            let value = match kind {
                "language" => borea_locale::LOCALES.iter().position(|v| v.id == n.locale.lang),
                "keyboard" => borea_locale::KEYBOARDS.iter().position(|v| {
                    v.layout == n.locale.keyboard.layout && v.variant == n.locale.keyboard.variant
                }),
                "console" => {
                    borea_locale::console_keymaps().position(|v| v == n.locale.keyboard.console)
                }
                _ => borea_locale::TIMEZONES.iter().position(|v| *v == n.locale.timezone),
            };
            let options = region_options(kind);
            let selected = value.and_then(|i| options.get(i)).cloned();
            column![
                pick_list(options, selected, move |s| Message::Region(kind, s))
                    .placeholder(hint)
                    .width(Length::Fill)
                    .text_size(15)
                    .padding(iced::Padding { top: 2., right: 18., bottom: 6., left: 0. })
                    .style(move |_, _| pick_list::Style {
                        text_color: p.text,
                        placeholder_color: Color { a: 0.55, ..p.muted },
                        handle_color: p.ice,
                        background: Color::TRANSPARENT.into(),
                        border: Border::default(),
                    }),
                underline(p, self.focus == Some(kind)),
            ]
            .width(Length::Fill)
            .into()
        };
        let mut second = row![choose("keyboard", "Keyboard")].spacing(24);
        // The disk unlock keyboard only matters when the disk is encrypted.
        let console: Element<'_, Message> = if !n.encrypt {
            space().width(Length::Fill).into()
        } else if n.touched("console") {
            column![
                text("Disk unlock keyboard").size(12).color(p.muted),
                choose("console", "Disk unlock keyboard")
            ]
            .spacing(2)
            .width(Length::Fill)
            .into()
        } else {
            let same = if n.locale.keyboard.layout.is_empty() {
                "Disk unlock keyboard: same as keyboard".to_string()
            } else {
                format!("Disk unlock keyboard: same ({})", n.locale.keyboard.console)
            };
            column![
                row![
                    text(same)
                        .size(15)
                        .wrapping(text::Wrapping::None)
                        .color(Color { a: 0.75, ..p.muted }),
                    button(text("Change").size(12).color(p.ice))
                        .padding(0)
                        .on_press(Message::ConsoleChange)
                        .style(|_, _| button::Style::default()),
                ]
                .spacing(8)
                .align_y(Alignment::Center)
                .padding(iced::Padding {
                    top: 2.,
                    bottom: 6.,
                    ..Default::default()
                }),
                underline(p, false),
            ]
            .width(Length::Fill)
            .into()
        };
        second = second.push(console);
        let grid = column![
            row![choose("language", "Language"), choose("timezone", "Time zone")].spacing(24),
            second
        ]
        .spacing(10);
        let words = column![
            self.form_title("Region"),
            container(grid).padding(iced::Padding { top: 12., right: 16., bottom: 14., left: 16. })
        ];
        let active = matches!(self.focus, Some("language" | "keyboard" | "console" | "timezone"));
        self.form_card(words.into(), active)
    }
    fn apps_card(&self) -> Element<'_, Message> {
        let n = &self.bridge.app.native;
        let p = self.colors();
        let a = &self.bridge.app;
        let extras = a
            .selection
            .ticked(&a.catalog, a.family())
            .into_iter()
            .map(|o| o.extra.name.as_str())
            .collect::<Vec<_>>();
        let picked: Vec<&str> = extras
            .into_iter()
            .chain(
                borea_catalog::APPS
                    .iter()
                    .filter(|app| n.apps.contains(app.id) && self.included_by(app).is_none())
                    .map(|app| app.name),
            )
            .collect();
        let count = match picked.len() {
            0 => "None".to_string(),
            1..=3 => picked.join(", "),
            more => format!("{} and {} more", picked[..3].join(", "), more - 3),
        };
        let head =
            |label: Element<'static, Message>, summary: String, open: bool, press: Message| {
                button(
                    row![
                        label,
                        space().width(Length::Fill),
                        text(summary).size(12).wrapping(text::Wrapping::None).color(p.ice),
                        chevron(p, open)
                    ]
                    .spacing(10)
                    .align_y(Alignment::Center),
                )
                .width(Length::Fill)
                .on_press(press)
                .style(|_, _| button::Style::default())
            };
        let mut words = column![
            head(
                text("Extra apps").size(15).font(TITLE).color(p.text).into(),
                count,
                self.apps_open,
                Message::Apps(!self.apps_open)
            )
            .padding([13, 16])
        ];
        if self.apps_open {
            // System extras first: chosen here and nowhere else in the installer.
            let offered: Vec<_> = a.catalog.extras.on(&a.distro().id).collect();
            if !offered.is_empty() {
                let mut list = column![].spacing(0);
                for pair in offered.chunks(2) {
                    let mut line = row![].spacing(40);
                    for o in pair {
                        let on = o.provision.built_in
                            || a.selection.extra(&a.catalog, a.family(), &o.extra.id);
                        let press = (!o.provision.built_in && !self.bridge.busy())
                            .then(|| Message::Choose(format!("extra/{}", o.extra.id)));
                        let note = if o.provision.built_in {
                            "included".to_owned()
                        } else {
                            o.summary().to_owned()
                        };
                        line = line.push(self.pick_row(&o.extra.name, Some(note), on, press));
                    }
                    if pair.len() < 2 {
                        line = line.push(space().width(Length::Fill));
                    }
                    list = list.push(line);
                }
                words = words
                    .push(divider(p, 0.65))
                    .push(
                        container(text("System extras").size(14).font(TITLE).color(p.text))
                            .padding([10, 16]),
                    )
                    .push(divider(p, 0.45))
                    .push(
                        container(list)
                            .width(Length::Fill)
                            .padding(iced::Padding { top: 8., right: 16., bottom: 12., left: 44. })
                            .style(move |_| container::Style {
                                background: Some(mix(p.surface, p.bg, 0.4).into()),
                                ..Default::default()
                            }),
                    );
            }
            let last = borea_catalog::Category::ALL.len() - 1;
            for (index, category) in borea_catalog::Category::ALL.into_iter().enumerate() {
                let label = category.label();
                let open = self.categories.contains(label);
                let apps: Vec<_> =
                    borea_catalog::APPS.iter().filter(|app| app.category == category).collect();
                let summary = apps
                    .iter()
                    .filter(|app| n.apps.contains(app.id))
                    .map(|app| app.name)
                    .collect::<Vec<_>>()
                    .join(", ");
                let name = text(label).size(14).font(if open { TITLE } else { FACE }).color(p.text);
                words = words.push(divider(p, if index == 0 { 0.65 } else { 0.6 })).push(
                    container(
                        head(name.into(), summary, open, Message::Category(label)).padding([10, 0]),
                    )
                    .padding([0, 16]),
                );
                if open {
                    let mut list = column![].spacing(0);
                    for pair in apps.chunks(2) {
                        let mut line = row![].spacing(40);
                        for app in pair {
                            line = line.push(self.app_row(app));
                        }
                        if pair.len() < 2 {
                            line = line.push(space().width(Length::Fill));
                        }
                        list = list.push(line);
                    }
                    let round = if index == last { 15. } else { 0. };
                    words = words.push(divider(p, 0.45)).push(
                        container(list)
                            .width(Length::Fill)
                            .padding(iced::Padding { top: 8., right: 16., bottom: 12., left: 44. })
                            .style(move |_| container::Style {
                                background: Some(mix(p.surface, p.bg, 0.4).into()),
                                border: Border {
                                    radius: iced::border::Radius {
                                        top_left: 0.,
                                        top_right: 0.,
                                        bottom_right: round,
                                        bottom_left: round,
                                    },
                                    ..Default::default()
                                },
                                ..Default::default()
                            }),
                    );
                }
            }
        }
        self.form_card(words.into(), self.apps_open)
    }
    /// One app: a round selector on the left, then its name. Apps with no source
    /// for this system stay faint and can't be picked.
    fn app_row(&self, app: &'static borea_catalog::App) -> Element<'_, Message> {
        // An app a ticked extra installs shows as included, not as a second choice.
        if let Some(extra) = self.included_by(app) {
            return self.pick_row(app.name, Some(format!("in {extra}")), true, None);
        }
        let p = self.colors();
        let on = self.bridge.app.native.apps.contains(app.id);
        let available = app_available(app);
        let ink = if on { p.text } else { p.muted };
        let tick =
            container(container(space()).width(7).height(7).style(move |_| container::Style {
                background: on.then(|| p.ice.into()),
                border: Border { radius: 3.5.into(), ..Default::default() },
                ..Default::default()
            }))
            .width(15)
            .height(15)
            .align_x(Alignment::Center)
            .align_y(Alignment::Center)
            .style(move |_| container::Style {
                border: Border {
                    color: if on { p.ice } else { Color { a: 0.4, ..p.muted } },
                    width: 1.5,
                    radius: 7.5.into(),
                },
                ..Default::default()
            });
        button(row![tick, text(app.name).size(14)].spacing(12).align_y(Alignment::Center))
            .width(Length::Fill)
            .padding([6, 0])
            .on_press_maybe(
                (available && !self.bridge.busy())
                    .then(|| Message::Choose(format!("app/{}", app.id))),
            )
            .style(move |_, status| button::Style {
                background: None,
                // Captures draw every button as Disabled, so availability decides.
                text_color: match status {
                    _ if !available => Color { a: 0.45, ..p.muted },
                    button::Status::Hovered | button::Status::Pressed => p.text,
                    _ => ink,
                },
                ..Default::default()
            })
            .into()
    }
    /// The ticked extra that already installs this app, if any.
    fn included_by(&self, app: &borea_catalog::App) -> Option<String> {
        let a = &self.bridge.app;
        a.selection.ticked(&a.catalog, a.family()).into_iter().find_map(|o| {
            app.native
                .iter()
                .any(|n| o.provision.packages.iter().any(|p| p == n.pkg))
                .then(|| o.extra.name.clone())
        })
    }
    /// A row with a round selector, a name and, faintly, a note. Without a press
    /// it can't be changed here: included, or not available.
    fn pick_row(
        &self,
        name: &str,
        note: Option<String>,
        on: bool,
        press: Option<Message>,
    ) -> Element<'_, Message> {
        let p = self.colors();
        let enabled = press.is_some();
        let tick =
            container(container(space()).width(7).height(7).style(move |_| container::Style {
                background: on.then(|| p.ice.into()),
                border: Border { radius: 3.5.into(), ..Default::default() },
                ..Default::default()
            }))
            .width(15)
            .height(15)
            .align_x(Alignment::Center)
            .align_y(Alignment::Center)
            .style(move |_| container::Style {
                border: Border {
                    color: if on { p.ice } else { Color { a: 0.4, ..p.muted } },
                    width: 1.5,
                    radius: 7.5.into(),
                },
                ..Default::default()
            });
        let mut words =
            row![tick, text(name.to_owned()).size(14)].spacing(12).align_y(Alignment::Center);
        if let Some(note) = note {
            words = words.push(text(note).size(12).color(Color { a: 0.7, ..p.muted }));
        }
        let ink = if on { p.text } else { p.muted };
        button(words)
            .width(Length::Fill)
            .padding([6, 0])
            .on_press_maybe(press)
            .style(move |_, status| button::Style {
                background: None,
                text_color: match status {
                    _ if !enabled && !on => Color { a: 0.45, ..p.muted },
                    button::Status::Hovered | button::Status::Pressed => p.text,
                    _ => ink,
                },
                ..Default::default()
            })
            .into()
    }
    fn wifi(&self) -> Element<'_, Message> {
        let p = self.colors();
        let mut content = column![
            text(
                self.bridge
                    .app
                    .wifi
                    .message
                    .clone()
                    .or(self.bridge.app.wifi.unavailable.clone())
                    .unwrap_or_default()
            )
            .size(13)
            .color(p.muted),
            checkbox(self.hidden).label("Hidden network").on_toggle(Message::Hidden)
        ]
        .spacing(12);
        if self.hidden {
            content = content.push(
                row![
                    text_input("Network name", &self.hidden_ssid)
                        .on_input(Message::HiddenSsid)
                        .padding(12)
                        .style(move |_, status| input_style(p, status)),
                    pick_list(
                        vec![
                            "WPA2 Personal".to_string(),
                            "WPA3 Personal".to_string(),
                            "Open".to_string()
                        ],
                        Some(self.hidden_security.clone()),
                        Message::HiddenSecurity
                    )
                ]
                .spacing(12),
            );
        }
        if self.hidden || self.bridge.network.is_some() {
            let secured = !matches!(
                self.network_security(),
                borea_network::Security::Open | borea_network::Security::EnhancedOpen
            );
            let mut join = row![].spacing(10).align_y(Alignment::Center);
            if secured {
                join = join.push(
                    text_input("Passphrase", &self.wifi_password)
                        .secure(!self.reveal)
                        .on_input(Message::WifiPassword)
                        .on_submit_maybe(self.join_valid().then_some(Message::Join))
                        .padding(12)
                        .style(move |_, status| input_style(p, status)),
                );
            }
            if self.join_valid() {
                join = join.push(button("Join network").on_press(Message::Join).padding(12));
            }
            content = content.push(join);
            if secured {
                content = content.push(
                    checkbox(self.reveal).label("Show passphrase").on_toggle(Message::Reveal),
                );
            }
        }
        content.into()
    }
    fn review<'a>(&'a self, mut body: widget::Column<'a, Message>) -> widget::Column<'a, Message> {
        let a = &self.bridge.app;
        let p = self.colors();
        let (disks, pick) = if self.bridge.native() {
            (&a.native.disks, &a.native.pick)
        } else {
            (&a.handoff.disks, &a.handoff.pick)
        };
        if let Some(Where::Alongside { disk, part, size }) = pick
            && let Some(max) =
                disks.get(*disk).and_then(|d| d.parts.get(*part)).and_then(|p| p.max_linux_size)
        {
            let minimum = if self.bridge.native() { 40. } else { 32. };
            body =
                body.push(text(format!("Linux space: {} GiB", size / handoff::GIB)).size(13)).push(
                    slider(
                        minimum..=(max / handoff::GIB) as f32,
                        (*size / handoff::GIB) as f32,
                        Message::Space,
                    )
                    .step(1_f32),
                );
        }
        if self.bridge.native() {
            let n = &a.native;
            if let Some(e) = &n.error
                && n.page != Page::Account
            {
                body = body.push(text(e.clone()).color(p.warn));
            }
            match n.page {
                Page::Storage if n.disks.is_empty() => {
                    body = body.push(text("Looking for available drives…").size(14))
                }
                Page::Review => {
                    body = body.push(self.facts(n.review_details().unwrap_or_default())).push(
                        checkbox(n.confirmed)
                            .label("Apply these disk changes. I have backed up anything important.")
                            .on_toggle(Message::Confirm),
                    );
                }
                Page::Running => {
                    body =
                        body.push(text(n.stage.clone())).push(progress_bar(0.0..=1.0, n.progress))
                }
                Page::Finished => body = body.push(text(
                    "Restart, unlock your disk and sign in. Extra apps finish quietly after login.",
                )),
                _ => {}
            }
        } else if a.handoff_target().is_some() {
            match &a.handoff.phase {
                Phase::Reviewed(review) => {
                    body=body.push(self.facts(format!("{}\n\n{}",review.steps.join("\n"),review.choose)))
                        .push(checkbox(a.handoff.confirmed).label("Apply these disk changes. I have backed up anything important.").on_toggle(Message::Confirm));
                }
                Phase::Running { stage, fraction, .. } => {
                    body = body
                        .push(text(stage.clone()))
                        .push(progress_bar(0.0..=1.0, fraction.unwrap_or(0.)))
                }
                Phase::Finished { choose } => body = body.push(text(choose.clone())),
                Phase::Failed(error) => body = body.push(text(error.clone())),
                _ => {}
            }
        } else {
            body = body.push(
                text(
                    "Arch, CachyOS and Tumbleweed install with Noctalia v5; Omarchy uses its own installer. This combination is not connected yet.",
                )
                .color(p.muted),
            );
        }
        if !self.bridge.busy() && a.handoff_target().is_none() {
            let available = matches!(a.secure_boot(), Some(Readiness::Available { .. }));
            let mut control =
                checkbox(a.selection.secure_boot).label("Enable Secure Boot (optional)");
            if available {
                control = control.on_toggle(|v| Message::Action(Action::SecureBoot(v)));
            }
            body = body.push(control).push(
                text(match a.secure_boot() {
                    Some(Readiness::Unavailable(why)) => format!("Leave Secure Boot off: {why}."),
                    _ => "Works with Secure Boot off. Firmware support is optional.".into(),
                })
                .size(12)
                .color(p.muted),
            );
        }
        if a.handoff_target().is_some() && !self.bridge.busy() {
            body = body.push(text("Omarchy's installer manages its boot settings. Borea's handoff requires Secure Boot off.").size(12).color(p.muted));
        }
        body
    }
}
/// Whether an app has a source on this system (Arch, preferring the AUR).
fn app_available(app: &borea_catalog::App) -> bool {
    matches!(
        borea_catalog::resolve(
            app,
            &borea_catalog::Sources::defaults(borea_catalog::Distro::Arch),
            borea_catalog::Prefer::Aur
        ),
        borea_catalog::Resolution::From {
            source: borea_catalog::Source::Repo(_)
                | borea_catalog::Source::Aur
                | borea_catalog::Source::Flathub,
            ..
        }
    )
}
/// The show-password eye.
const EYE: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="black" stroke-width="1.6"><path d="M2 12s3.6-6.5 10-6.5S22 12 22 12s-3.6 6.5-10 6.5S2 12 2 12Z"/><circle cx="12" cy="12" r="3"/></svg>"#;
/// A drop-down's chevron: down when closed, up when open.
fn chevron<'a>(p: Colors, open: bool) -> Element<'a, Message> {
    let path = if open { "M3 9.5 8 4.5 13 9.5" } else { "M3 5.5 8 10.5 13 5.5" };
    let mark = format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 16 16" fill="none" stroke="black" stroke-width="1.5"><path d="{path}"/></svg>"#
    );
    svg(svg::Handle::from_memory(mark.into_bytes()))
        .width(12)
        .height(12)
        .style(move |_, _| svg::Style { color: Some(p.ice) })
        .into()
}
/// A full-width hairline between a card's sections, `strength` of the line colour.
fn divider<'a>(p: Colors, strength: f32) -> Element<'a, Message> {
    container(space())
        .width(Length::Fill)
        .height(1)
        .style(move |_| container::Style {
            background: Some(Color { a: strength, ..p.line }.into()),
            ..Default::default()
        })
        .into()
}
/// The thin line under a field, ice while it's the one being filled in.
fn underline<'a>(p: Colors, on: bool) -> Element<'a, Message> {
    container(space())
        .width(Length::Fill)
        .height(1)
        .style(move |_| container::Style {
            background: Some(if on { p.ice } else { Color { a: 0.3, ..p.muted } }.into()),
            ..Default::default()
        })
        .into()
}
/// A field with no box: its card and line frame it.
fn bare_input(p: Colors) -> text_input::Style {
    text_input::Style {
        background: Color::TRANSPARENT.into(),
        border: Border::default(),
        icon: p.muted,
        placeholder: Color { a: 0.55, ..p.muted },
        value: p.text,
        selection: Color { a: 0.35, ..p.ice },
    }
}
/// Small words under a field.
fn note<'a>(words: String, colour: Color) -> Element<'a, Message> {
    text(words).size(12).color(colour).into()
}
/// "Passwords match." once they do, "Doesn't match yet." once they can't.
fn match_note<'a>(p: Colors, password: &str, confirmation: &str) -> Element<'a, Message> {
    match crate::model::matches(password, confirmation) {
        Some(true) => note("Passwords match.".into(), p.ice),
        Some(false) => note("Doesn\u{2019}t match yet.".into(), p.warn),
        None => space().into(),
    }
}
/// The ice switch: line-grey when off, ice when on.
fn switch(p: Colors, status: toggler::Status) -> toggler::Style {
    // Before its first event a toggler reports Disabled; its state still shows.
    let (toggler::Status::Active { is_toggled: on }
    | toggler::Status::Hovered { is_toggled: on }
    | toggler::Status::Disabled { is_toggled: on }) = status;
    toggler::Style {
        background: if on { p.ice } else { Color { a: 0.85, ..p.line } }.into(),
        background_border_width: 0.,
        background_border_color: Color::TRANSPARENT,
        foreground: if on { p.bg } else { p.text }.into(),
        foreground_border_width: 0.,
        foreground_border_color: Color::TRANSPARENT,
        text_color: None,
        border_radius: None,
        padding_ratio: 0.1,
    }
}
fn region_options(kind: &str) -> Vec<String> {
    match kind {
        "language" => {
            borea_locale::LOCALES.iter().map(|v| format!("{} · {}", v.english, v.id)).collect()
        }
        "keyboard" => borea_locale::KEYBOARDS.iter().map(|v| v.description.to_string()).collect(),
        "console" => borea_locale::console_keymaps().map(str::to_string).collect(),
        _ => borea_locale::TIMEZONES.iter().map(|s| s.to_string()).collect(),
    }
}
#[derive(Clone, Copy)]
struct Colors {
    bg: Color,
    surface: Color,
    text: Color,
    muted: Color,
    ice: Color,
    line: Color,
    selected: Color,
    /// Errors, inline under what they're about.
    warn: Color,
}
impl Colors {
    fn new(dark: bool) -> Self {
        let rgb = Color::from_rgb8;
        if dark {
            Self {
                bg: rgb(35, 42, 54),
                surface: rgb(46, 52, 64),
                text: rgb(236, 239, 244),
                muted: rgb(180, 193, 209),
                ice: rgb(136, 192, 208),
                line: rgb(76, 86, 106),
                selected: rgb(48, 76, 94),
                warn: rgb(208, 135, 112),
            }
        } else {
            Self {
                bg: rgb(236, 239, 244),
                surface: rgb(229, 233, 240),
                text: rgb(46, 52, 64),
                muted: rgb(76, 86, 106),
                ice: rgb(46, 99, 126),
                line: rgb(170, 187, 203),
                selected: rgb(206, 227, 237),
                warn: rgb(166, 82, 46),
            }
        }
    }
}
/// `a` moved `t` (0 to 1) of the way to `b`.
fn mix(a: Color, b: Color, t: f32) -> Color {
    let t = t.clamp(0., 1.);
    let at = |x: f32, y: f32| x + (y - x) * t;
    Color { r: at(a.r, b.r), g: at(a.g, b.g), b: at(a.b, b.b), a: at(a.a, b.a) }
}
/// The three card sizes, in one language.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Size {
    /// Family and distribution cards: logo, name, own words and release.
    Hero,
    /// Every other choice: name, detail and tag.
    Compact,
    /// The step rail.
    Rail,
    /// A titled card of fields: Account, Region, Extra apps. Its sections pad themselves.
    Form,
}
impl Size {
    fn padding(self) -> f32 {
        match self {
            Size::Hero => 22.,
            Size::Compact => 14.,
            Size::Rail => 12.,
            Size::Form => 0.,
        }
    }
    fn height(self) -> Length {
        match self {
            Size::Hero => Length::Fixed(245.),
            Size::Compact | Size::Rail | Size::Form => Length::Shrink,
        }
    }
    /// How far it rises under the pointer, in points.
    fn rise(self) -> f32 {
        match self {
            Size::Hero => 4.,
            Size::Compact | Size::Rail => 2.,
            Size::Form => 0.,
        }
    }
    /// How far its glow spreads, in points.
    fn glow(self) -> f32 {
        match self {
            Size::Hero => 32.,
            Size::Compact | Size::Form => 22.,
            Size::Rail => 18.,
        }
    }
}
/// What a card is, beyond its words.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Look {
    /// A choice: it lifts under the pointer and washes in its accent once picked.
    Ready,
    /// Not ready to pick: sunk toward the background, flat and still.
    Waiting,
    /// Facts to read: a plain card, no hover and no greying.
    Info,
    /// Fields to fill in: a plain card that takes the ice rim while you type in it.
    Form,
}
/// The rail key for a step, so it eases like any other card.
fn step_key(step: Step) -> String {
    format!("step/{}", step.number())
}
/// A quiet tag: "Coming soon", "Not available", "Included", or a status.
fn tag<'a>(p: Colors, label: String) -> Element<'a, Message> {
    container(text(label).size(11.5).wrapping(text::Wrapping::None).color(p.muted))
        .height(22)
        .padding([0, 8])
        .align_y(Alignment::Center)
        .style(move |_| container::Style {
            background: Some(Color { a: 0.16, ..p.muted }.into()),
            border: Border { radius: 6.into(), ..Default::default() },
            ..Default::default()
        })
        .into()
}
/// A card's surface. `lift` and `chosen` are eased 0 to 1; `angle` is the picked
/// wash's direction in degrees (CSS convention: 0 up, 90 right).
#[allow(clippy::too_many_arguments)]
fn card_surface(
    p: Colors,
    brand: Color,
    shade: Color,
    depth: (f32, f32),
    look: Look,
    lift: f32,
    chosen: f32,
    angle: f32,
) -> container::Style {
    match look {
        Look::Waiting => {
            return container::Style {
                background: Some(mix(p.surface, p.bg, 0.45).into()),
                border: Border { color: mix(p.line, p.bg, 0.3), width: 1., radius: 16.into() },
                ..Default::default()
            };
        }
        Look::Info => {
            return container::Style {
                background: Some(p.surface.into()),
                border: Border { color: p.line, width: 1., radius: 16.into() },
                ..Default::default()
            };
        }
        Look::Form => {
            return container::Style {
                background: Some(p.surface.into()),
                border: Border { color: mix(p.line, brand, chosen), width: 1., radius: 16.into() },
                ..Default::default()
            };
        }
        Look::Ready => {}
    }
    let wash = |amount: f32| mix(p.surface, brand, amount * chosen);
    let background = if chosen > 0.001 {
        Background::Gradient(iced::Gradient::Linear(
            iced::gradient::Linear::new(iced::Degrees(angle))
                .add_stop(0., wash(0.34))
                .add_stop(0.55, wash(0.12))
                .add_stop(1., p.surface),
        ))
    } else {
        Background::Color(p.surface)
    };
    container::Style {
        background: Some(background),
        border: Border {
            color: mix(mix(p.line, brand, 0.6 * lift), brand, chosen),
            width: 1. + chosen,
            radius: 16.into(),
        },
        shadow: iced::Shadow {
            color: Color { a: depth.0 + depth.1 * lift, ..shade },
            offset: iced::Vector::new(0., 2. + 12. * lift),
            blur_radius: 6. + 24. * lift,
        },
        ..Default::default()
    }
}
/// Fedora's logo, Borea's one raster logo. One handle for the whole run: a new
/// handle each frame has a new id, so the renderer decodes it again every frame
/// and it flickers.
static FEDORA: std::sync::LazyLock<image::Handle> = std::sync::LazyLock::new(|| {
    image::Handle::from_bytes(include_bytes!("../ui/logos/fedora.png").as_slice())
});
/// A logo at `opacity`, its artwork untouched.
fn faded_logo(id: &str, dark: bool, opacity: f32) -> Option<Element<'static, Message>> {
    let bytes: &'static [u8] = match id {
        "arch" => include_bytes!("../ui/logos/arch.svg"),
        "debian" => include_bytes!("../ui/logos/debian.svg"),
        "opensuse" | "tumbleweed" => include_bytes!("../ui/logos/opensuse.svg"),
        "cachyos" => include_bytes!("../ui/logos/cachyos.svg"),
        "omarchy" => include_bytes!("../ui/logos/omarchy.svg"),
        "bazzite" => include_bytes!("../ui/logos/bazzite.svg"),
        "nobara" => include_bytes!("../ui/logos/nobara.svg"),
        "pikaos" => include_bytes!("../ui/logos/pikaos.svg"),
        "fedora" => {
            return Some(image(FEDORA.clone()).width(52).height(52).opacity(opacity).into());
        }
        _ => return None,
    };
    let mark = svg(svg::Handle::from_memory(bytes))
        .width(if id == "omarchy" { 96 } else { 52 })
        .height(52)
        .opacity(opacity);
    let mark = if id == "omarchy" {
        mark.style(move |_, _| svg::Style {
            color: Some(if dark { Color::WHITE } else { Color::BLACK }),
        })
    } else {
        mark
    };
    Some(mark.into())
}

struct AirCanvas {
    trails: Vec<crate::frost_motion::Trail>,
    dark: bool,
}
impl widget::canvas::Program<Message> for AirCanvas {
    type State = ();
    fn draw(
        &self,
        _: &(),
        renderer: &iced::Renderer,
        _: &Theme,
        bounds: iced::Rectangle,
        _: iced::mouse::Cursor,
    ) -> Vec<widget::canvas::Geometry> {
        use widget::canvas::{Frame, Path, Stroke};
        let mut f = Frame::new(renderer, bounds.size());
        let p = Colors::new(self.dark);
        f.fill_rectangle(iced::Point::ORIGIN, bounds.size(), p.bg);
        // Each line is painted in short runs, so it can blur away behind its head.
        for g in &self.trails {
            for run in g.points.windows(2) {
                let alpha = (run[0].alpha + run[1].alpha) / 2.;
                if alpha < 0.004 {
                    continue;
                }
                let y = run[0].y * bounds.height;
                let path = Path::line(
                    iced::Point::new(run[0].x * bounds.width, y),
                    iced::Point::new(run[1].x * bounds.width, y),
                );
                f.stroke(
                    &path,
                    Stroke::default().with_color(Color { a: alpha, ..p.ice }).with_width(1.3),
                );
            }
        }
        vec![f.into_geometry()]
    }
}
/// Drawn on a card that can be picked, beneath its words. The side under the pointer gives
/// way: it sinks into shade while the far side catches a little light, as far as the
/// card has lifted. Just picked, a faint bloom of the family colour grows from the pointer.
struct CardFx {
    /// The pointer over this card, from its top-left corner. (The words drawn above
    /// this layer take the live cursor, so it comes from the card's mouse area.)
    at: Option<iced::Point>,
    lift: f32,
    push: bool,
    bloom: f32,
    brand: Color,
    shade: Color,
}
impl widget::canvas::Program<Message> for CardFx {
    type State = ();
    fn draw(
        &self,
        _: &(),
        renderer: &iced::Renderer,
        _: &Theme,
        bounds: iced::Rectangle,
        _: iced::mouse::Cursor,
    ) -> Vec<widget::canvas::Geometry> {
        use iced::Point;
        use widget::canvas::{Fill, Frame, Gradient, Path, Style, gradient::Linear};
        let mut f = Frame::new(renderer, bounds.size());
        let (w, h) = (bounds.width, bounds.height);
        let card = Path::rounded_rectangle(Point::ORIGIN, bounds.size(), 16.into());
        let at = self.at;
        if self.push
            && self.lift > 0.01
            && let Some(at) = at
        {
            // -1 at the left or top edge, 1 at the right or bottom edge.
            let across = (at.x / w * 2. - 1.).clamp(-1., 1.);
            let down = (at.y / h * 2. - 1.).clamp(-1., 1.);
            for (lean, start, end) in [
                (
                    across,
                    Point::new(if across > 0. { w } else { 0. }, 0.),
                    Point::new(if across > 0. { 0. } else { w }, 0.),
                ),
                (
                    down,
                    Point::new(0., if down > 0. { h } else { 0. }),
                    Point::new(0., if down > 0. { 0. } else { h }),
                ),
            ] {
                let give = lean.abs() * self.lift;
                if give < 0.01 {
                    continue;
                }
                let fall = Linear::new(start, end)
                    .add_stop(0., Color { a: 0.34 * give, ..self.shade })
                    .add_stop(0.55, Color { a: 0., ..self.shade })
                    .add_stop(0.8, Color { a: 0., ..Color::WHITE })
                    .add_stop(1., Color { a: 0.08 * give, ..Color::WHITE });
                f.fill(
                    &card,
                    Fill { style: Style::Gradient(Gradient::Linear(fall)), ..Fill::default() },
                );
            }
        }
        if self.bloom > 0. {
            let from = at.unwrap_or(Point::new(w / 2., h / 2.));
            let grown = 1. - self.bloom.powi(3);
            let reach = w.hypot(h) * (0.15 + 0.85 * grown);
            f.with_clip(iced::Rectangle::with_size(bounds.size()), |f| {
                for ring in 0..6 {
                    let r = reach * (1. - ring as f32 / 6.);
                    f.fill(&Path::circle(from, r), Color { a: 0.05 * self.bloom, ..self.brand });
                }
            });
        }
        vec![f.into_geometry()]
    }
}
pub fn run(live: bool) -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().collect();
    if let Some(i) = args.iter().position(|a| a == "--showcase") {
        let dir = args.get(i + 1).ok_or("--showcase needs an output directory")?;
        return showcase(std::path::Path::new(dir), args.iter().any(|a| a == "--animate"));
    }
    let boot_smoke = args.iter().any(|a| a == "--boot-smoke");
    let windowed = args.iter().any(|a| a == "--window");
    let mut application =
        iced::application(move || App::new(live, boot_smoke), App::update, App::view)
            .title(if live { "Borea Live" } else { "Windpad" })
            .theme(App::theme)
            .subscription(App::subscription)
            .font(FONT)
            .font(TITLE_FONT);
    for font in BRAND_FONTS {
        application = application.font(font);
    }
    application
        .default_font(FACE)
        .window(iced::window::Settings {
            size: iced::Size::new(1120., 753.),
            min_size: Some(iced::Size::new(900., 600.)),
            fullscreen: live && !windowed,
            ..Default::default()
        })
        .run()?;
    Ok(())
}

#[path = "../tests/common/mod.rs"]
mod showcase_fixture;
/// Render the real Iced widget tree with the same CPU renderer used on the ISO.
fn capture(
    app: &App,
    path: &std::path::Path,
    width: u32,
    height: u32,
) -> Result<(), Box<dyn std::error::Error>> {
    capture_at(app, path, width, height, iced::mouse::Cursor::Unavailable)
}
/// As `capture`, with the pointer at `cursor`.
fn capture_at(
    app: &App,
    path: &std::path::Path,
    width: u32,
    height: u32,
    cursor: iced::mouse::Cursor,
) -> Result<(), Box<dyn std::error::Error>> {
    use iced_runtime::user_interface::{Cache, UserInterface};
    // Screenshots always draw on the CPU, the same way in any environment.
    let mut renderer =
        iced::Renderer::Secondary(iced_tiny_skia::Renderer::new(FACE, iced::Pixels(14.)));
    let size = iced::Size::new(width as f32, height as f32);
    let mut ui = UserInterface::build(app.view(), size, Cache::new(), &mut renderer);
    if app.bridge.native() && app.bridge.app.native.page == Page::Account && app.apps_open {
        // Scroll the real account view down to its open apps, as a user does.
        let y = if app.elapsed > 0. {
            900. * ((app.elapsed - 21.2) / 1.4).clamp(0., 1.)
        } else {
            2000.
        };
        ui.operate(
            &renderer,
            &mut iced::advanced::widget::operation::scrollable::scroll_to::<()>(
                iced::advanced::widget::Id::new("installer-body"),
                iced::advanced::widget::operation::scrollable::AbsoluteOffset {
                    x: None,
                    y: Some(y),
                },
            ),
        );
    }
    ui.draw(
        &mut renderer,
        &app.theme(),
        &iced::advanced::renderer::Style { text_color: app.colors().text },
        cursor,
    );
    // The tree stays alive until the pixels are drawn: tiny-skia keeps only weak
    // references to its text, so dropping it first draws every label blank.
    let iced::Renderer::Secondary(renderer) = &mut renderer else {
        return Err("screenshots draw with tiny-skia".into());
    };
    let mut pixels =
        tiny_skia::Pixmap::new(width, height).ok_or("Invalid screenshot dimensions")?;
    let mut mask = tiny_skia::Mask::new(width, height).ok_or("Invalid clip dimensions")?;
    renderer.draw(
        &mut pixels.as_mut(),
        &mut mask,
        &iced_tiny_skia::graphics::Viewport::with_physical_size(iced::Size::new(width, height), 1.),
        &[iced::Rectangle::with_size(size)],
        app.colors().bg,
    );
    drop(ui);
    // tiny-skia's window renderer writes BGRA for softbuffer. PNG expects RGBA.
    for pixel in pixels.data_mut().as_chunks_mut::<4>().0 {
        pixel.swap(0, 2);
    }
    pixels.save_png(path)?;
    Ok(())
}
fn showcase_disks() -> Vec<handoff::Disk> {
    serde_json::from_value(serde_json::json!([{
        "id":{"logical_sector":512,"path":"/dev/vda","serial":"SHOWCASE","size":512_000_000_000u64,"model":"Example NVMe"},
        "readonly":false,"in_use":false,"live":false,"parts":[{"path":"/dev/vda3","number":3,"uuid":"SHOWCASE-NTFS","fs":"ntfs","max_linux_size":128_849_018_880u64}]
    }])).unwrap_or_default()
}
/// The example account: the username follows the name; the hostname is typed.
fn showcase_account(app: &mut App) {
    for (field, value) in [
        ("full-name", "Alex Morgan"),
        ("hostname", "frost"),
        ("password", "Showcase-only-1"),
        ("confirmation", "Showcase-only-1"),
    ] {
        let _ = app.update(Message::Field(field, value.into()));
    }
    showcase_region(app);
}
/// The region as the connection suggests it.
fn showcase_region(app: &mut App) {
    let locale = borea_core::LocaleChoice {
        lang: "en_GB.UTF-8".into(),
        timezone: "Europe/London".into(),
        keyboard: borea_core::Keyboard {
            layout: "gb".into(),
            variant: String::new(),
            console: "uk".into(),
        },
    };
    app.bridge.app.native.locale = locale;
}
/// Extra apps open with the Gaming extra (Steam and Lutris shown as in Gaming),
/// Chat & email and Games open, Vesktop and Firefox picked.
fn showcase_apps(app: &mut App) {
    app.bridge.command("extra/gaming");
    app.bridge.command("app/firefox");
    app.bridge.command("app/vesktop");
    app.apps_open = true;
    app.categories.insert(borea_catalog::Category::ChatEmail.label());
    app.categories.insert(borea_catalog::Category::Games.label());
}
/// Bundled fonts for captures, which draw without a window to load them.
fn load_fonts() -> Result<(), Box<dyn std::error::Error>> {
    use std::borrow::Cow;
    let mut fonts = iced_tiny_skia::graphics::text::font_system()
        .write()
        .map_err(|_| "Font system lock failed")?;
    for font in [FONT, TITLE_FONT].into_iter().chain(BRAND_FONTS) {
        fonts.load_font(Cow::Borrowed(font));
    }
    Ok(())
}
fn showcase(dir: &std::path::Path, animate: bool) -> Result<(), Box<dyn std::error::Error>> {
    load_fonts()?;
    std::fs::create_dir_all(dir.join("motion"))?;
    let make = |setup_mode: bool| {
        let wifi = WifiLink::simulated(
            borea_network::Status {
                connectivity: borea_network::Connectivity::None,
                radio_enabled: true,
                radio_unblocked: true,
                primary: None,
            },
            vec![borea_network::Network::sample(
                "Breeze Studio",
                86,
                borea_network::Security::Wpa3Personal,
                borea_network::Band::Ghz5,
                false,
            )],
        );
        let mut machine = showcase_fixture::machine();
        if setup_mode {
            machine.setup_mode = borea_live_core::Fact::Known(true);
        }
        let mut workflow = LiveInstaller::with_link(machine, wifi, true);
        // Capture fixtures cannot start installation, reboot or contact any service.
        workflow.set_install_service(std::sync::Arc::new(|method,_|match method {
            "plan.review"=>Ok(serde_json::json!({"approval":"showcase-only","review":{"packages":[{"name":"hyprland","source":"arch.core-extra"},{"name":"noctalia","source":"arch.core-extra"},{"name":"noctalia-greeter","source":"arch.core-extra"},{"name":"vesktop-bin","source":"aur"}],"warnings":["Example plan. This capture does not install a system."]}})),
            _=>Err("Unavailable in showcase mode".into()),
        }));
        let mut app = App::from_workflow(workflow, false);
        app.bridge.network = Some(0);
        app
    };
    let mut app = make(false);
    capture(&app, &dir.join("connect.png"), 1120, 753)?;
    app.bridge.app.go(Step::Machine);
    capture(&app, &dir.join("machine.png"), 1120, 753)?;
    app.bridge.app.go(Step::System);
    for dark in [true, false] {
        app.bridge.app.dark = dark;
        capture(
            &app,
            &dir.join(if dark { "family-dark.png" } else { "family-light.png" }),
            1120,
            753,
        )?;
    }
    app.bridge.app.dark = true;
    // Arch hovered, the pointer near its top-right corner, so that corner gives way.
    // Motion on for this one capture: paused, cards don't give way.
    let still = std::mem::replace(&mut app.bridge.app.still, false);
    app.hovered = Some("family/0".into());
    app.pointer = Some(iced::Point::new(396., 18.));
    app.lift.insert("family/0".into(), 1.);
    let corner = iced::mouse::Cursor::Available(iced::Point::new(640., 166.));
    capture_at(&app, &dir.join("family-hover.png"), 1120, 753, corner)?;
    app.hovered = None;
    app.pointer = None;
    app.lift.clear();
    app.bridge.app.still = still;
    app.bridge.command("family/0");
    // Back on the Family step with Arch picked: washed in Arch's colour.
    app.bridge.app.go(Step::System);
    app.chosen.insert("family/0".into(), 1.);
    for dark in [true, false] {
        app.bridge.app.dark = dark;
        let name = if dark { "family-picked-dark.png" } else { "family-picked-light.png" };
        capture(&app, &dir.join(name), 1120, 753)?;
    }
    capture(&app, &dir.join("family-small.png"), 900, 600)?;
    app.bridge.app.dark = true;
    app.bridge.app.go(Step::Distribution);
    app.bridge.command("distro/0");
    app.bridge.command("edition/3");
    capture(&app, &dir.join("distribution.png"), 1120, 753)?;
    app.bridge.next();
    app.bridge.command("mode/compose");
    app.bridge.command("compositor/hyprland");
    app.bridge.command("shell/noctalia");
    capture(&app, &dir.join("desktop.png"), 1120, 753)?;
    app.bridge.next();
    app.bridge.app.native.disks = showcase_disks();
    capture(&app, &dir.join("storage.png"), 1120, 753)?;
    app.bridge.command("whole/0");
    app.bridge.next();
    // Tumbleweed Minimal composes Noctalia v5 with any of the three compositors.
    let mut tumbleweed = make(false);
    tumbleweed.bridge.app.go(Step::System);
    let opensuse = Family::ALL.iter().position(|f| *f == Family::Opensuse).unwrap_or(3);
    tumbleweed.bridge.command(&format!("family/{opensuse}"));
    tumbleweed.bridge.command("distro/0");
    let minimal =
        tumbleweed.bridge.app.distro().editions.iter().position(|e| e.composable).unwrap_or(0);
    tumbleweed.bridge.command(&format!("edition/{minimal}"));
    capture(&tumbleweed, &dir.join("tumbleweed-distribution.png"), 1120, 753)?;
    tumbleweed.bridge.next();
    tumbleweed.bridge.command("mode/compose");
    tumbleweed.bridge.command("compositor/mango");
    tumbleweed.bridge.command("shell/noctalia");
    capture(&tumbleweed, &dir.join("tumbleweed-desktop.png"), 1120, 753)?;
    // The Account page, empty, filled, with a separate sudo password, and with its
    // apps open, in both themes; then encryption offered, on firmware in Setup Mode.
    let both = |app: &mut App, name: &str| -> Result<(), Box<dyn std::error::Error>> {
        for dark in [true, false] {
            app.bridge.app.dark = dark;
            let theme = if dark { "dark" } else { "light" };
            capture(app, &dir.join(format!("{name}-{theme}.png")), 1120, 753)?;
            capture(app, &dir.join(format!("{name}-{theme}-small.png")), 900, 600)?;
        }
        app.bridge.app.dark = true;
        Ok(())
    };
    both(&mut app, "account-empty")?;
    showcase_account(&mut app);
    app.focus = Some("password");
    both(&mut app, "account-filled")?;
    capture(&app, &dir.join("account.png"), 1120, 753)?;
    capture(&app, &dir.join("account-small.png"), 900, 600)?;
    let _ = app.update(Message::SameSudo(false));
    let _ = app.update(Message::Field("admin", "Sudo-only-2".into()));
    let _ = app.update(Message::Field("admin-confirmation", "Sudo-only".into()));
    both(&mut app, "account-sudo")?;
    let _ = app.update(Message::SameSudo(true));
    showcase_apps(&mut app);
    both(&mut app, "account-apps")?;
    app.apps_open = false;
    app.focus = None;
    let mut setup = make(true);
    setup.bridge.app.go(Step::System);
    setup.bridge.command("family/0");
    setup.bridge.app.go(Step::Distribution);
    setup.bridge.command("distro/0");
    setup.bridge.command("edition/3");
    setup.bridge.next();
    setup.bridge.command("mode/compose");
    setup.bridge.command("compositor/hyprland");
    setup.bridge.command("shell/noctalia");
    setup.bridge.next();
    setup.bridge.app.native.disks = showcase_disks();
    setup.bridge.command("whole/0");
    setup.bridge.next();
    showcase_account(&mut setup);
    both(&mut setup, "account-encrypt-offered")?;
    let _ = setup.update(Message::Encrypt(true));
    both(&mut setup, "account-encrypted")?;
    app.bridge.app.native.review();
    for _ in 0..100 {
        app.bridge.app.native.poll();
        if app.bridge.app.native.page == Page::Review {
            break;
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    capture(&app, &dir.join("review.png"), 1120, 753)?;
    if !animate {
        println!("Iced showcase captured in {}", dir.display());
        return Ok(());
    }
    app = make(false);
    app.bridge.app.still = false;
    let mut previous = String::new();
    for frame in 0..600 {
        if frame % 2 == 0 {
            match frame / 2 {
                8 => *app.wifi_password = "showcase-only-password".into(),
                20 => app.bridge.app.go(Step::Machine),
                36 => app.bridge.app.go(Step::System),
                44 => {
                    app.hovered = Some("family/0".into());
                    app.pointer = Some(iced::Point::new(396., 18.));
                }
                88 => {
                    app.bridge.pending_family = Some((Family::Arch, Instant::now()));
                    app.pulse = 1.;
                    app.bloom = Some("family/0".into());
                }
                92 => {
                    app.bridge.pending_family = None;
                    app.bridge.app.apply(Action::Family(Family::Arch));
                }
                102 => {
                    app.bridge.command("distro/0");
                    app.pulse = 1.;
                    app.bloom = Some("distro/0".into());
                }
                116 => app.bridge.command("edition/3"),
                128 => app.bridge.next(),
                136 => app.bridge.command("mode/compose"),
                146 => app.bridge.command("compositor/hyprland"),
                162 => {
                    app.bridge.command("shell/noctalia");
                    app.pulse = 1.;
                    app.bloom = Some("shell/noctalia".into());
                }
                190 => {
                    app.bridge.next();
                    app.bridge.app.native.disks = showcase_disks();
                }
                204 => {
                    app.bridge.command("whole/0");
                    app.bridge.next();
                }
                // The Account page fills in as someone types.
                218 => {
                    let _ = app.update(Message::Field("full-name", "Alex".into()));
                }
                221 => {
                    let _ = app.update(Message::Field("full-name", "Alex Morgan".into()));
                }
                226 => {
                    let _ = app.update(Message::Field("hostname", "frost".into()));
                }
                231 => {
                    let _ = app.update(Message::Field("password", "Showcase-only-1".into()));
                }
                236 => {
                    let _ = app.update(Message::Field("confirmation", "Showcase-only-1".into()));
                }
                240 => showcase_region(&mut app),
                248 => app.apps_open = true,
                // System extras live here, on the review, and nowhere else.
                250 => app.bridge.command("extra/gaming"),
                252 => {
                    app.categories.insert(borea_catalog::Category::ChatEmail.label());
                }
                258 => app.bridge.command("app/vesktop"),
                262 => app.bridge.command("app/firefox"),
                280 => app.bridge.app.native.review(),
                _ => {}
            }
        }
        if frame >= 560 {
            app.bridge.app.native.poll();
        }
        let next = app.key();
        if next != previous {
            // The hovered card left with its page.
            app.hovered = None;
            app.pointer = None;
        }
        app.ease_cards(1. / 24., false);
        app.wind.advance(1. / 24., next != previous, false, None);
        previous = next;
        app.elapsed += 1. / 24.;
        app.pulse = (app.pulse - 1. / 24. / 0.7).max(0.);
        let cursor = if app.hovered.as_deref() == Some("family/0") {
            iced::mouse::Cursor::Available(iced::Point::new(640., 166.))
        } else {
            iced::mouse::Cursor::Unavailable
        };
        capture_at(&app, &dir.join(format!("motion/frame-{frame:04}.png")), 1120, 753, cursor)?;
    }
    println!("Iced showcase captured in {}", dir.display());
    Ok(())
}
/// Borea Live draws on the CPU (no real GPU, a VM, or safe graphics): the
/// session says so, and motion is kept light.
fn software_graphics() -> bool {
    static SOFTWARE: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *SOFTWARE.get_or_init(|| std::env::var("BOREA_GRAPHICS").is_ok_and(|g| g == "software"))
}

/// How often the window wakes to poll and animate. Every wake redraws it, so
/// a still window wakes rarely, and on the CPU the motion runs at 15 frames.
fn tick(still: bool, software: bool) -> Duration {
    match (still, software) {
        (true, _) => Duration::from_millis(400),
        (false, true) => Duration::from_millis(66),
        (false, false) => crate::wind::FRAME,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn bridge() -> Bridge {
        Bridge {
            app: LiveInstaller::with_hardware(showcase_fixture::machine(), true),
            network: None,
            network_name: None,
            pending_family: None,
        }
    }
    fn native_path(b: &mut Bridge) {
        b.app.go(Step::System);
        b.command("family/0");
        b.command("distro/0");
        b.command("edition/3");
        b.next();
        b.command("mode/compose");
        b.command("compositor/hyprland");
        b.command("shell/noctalia");
        b.next();
        assert!(b.native());
    }
    #[test]
    fn iced_commands_require_every_choice_and_reset_later_choices() {
        let mut b = bridge();
        b.app.go(Step::System);
        b.next();
        assert_eq!(b.app.step, Step::System);
        b.command("family/0");
        b.next();
        assert_eq!(b.app.step, Step::Distribution);
        let (h, _) = b.tiles();
        assert!(h.iter().all(|t| !t.selected));
        b.command("distro/0");
        b.next();
        assert_eq!(b.app.step, Step::Distribution);
        b.command("edition/3");
        b.next();
        assert_eq!(b.app.step, Step::Desktop);
        b.command("mode/compose");
        b.command("compositor/hyprland");
        b.next();
        assert_eq!(b.app.step, Step::Desktop);
        b.command("shell/noctalia");
        b.next();
        assert!(b.native());
        assert!(b.app.native.pick.is_none());
        b.app.native.page = Page::Review;
        b.back();
        assert_eq!(b.app.native.page, Page::Account);
        b.app.go(Step::System);
        b.command("distro/1");
        assert!(b.app.picked_distro);
        assert!(!b.app.picked_edition);
    }
    #[test]
    fn iced_account_and_region_reach_the_reviewed_plan() {
        let mut b = bridge();
        native_path(&mut b);
        b.app.native.disks=serde_json::from_value(serde_json::json!([{
            "id":{"logical_sector":512,"path":"/dev/vda","serial":"TEST","size":107374182400u64,"model":"Disposable VM disk"},
            "readonly":false,"in_use":false,"live":false,"parts":[]
        }])).unwrap();
        b.command("whole/0");
        b.next();
        assert_eq!(b.app.native.page, Page::Account);
        for (key, value) in [
            ("full-name", "Test Person"),
            ("username", "tester"),
            ("hostname", "test-pc"),
            ("password", "disposable-test-only"),
            ("confirmation", "disposable-test-only"),
        ] {
            b.app.native.edit_account(key, value.into());
        }
        b.region(
            "language",
            borea_locale::LOCALES.iter().position(|v| v.id == "fr_FR.UTF-8").unwrap() as i32,
        );
        b.region(
            "keyboard",
            borea_locale::KEYBOARDS
                .iter()
                .position(|v| v.layout == "de" && v.variant == "nodeadkeys")
                .unwrap() as i32,
        );
        b.region(
            "console",
            borea_locale::console_keymaps().position(|v| v == "de-latin1").unwrap() as i32,
        );
        b.region(
            "timezone",
            borea_locale::TIMEZONES.iter().position(|v| *v == "Asia/Tokyo").unwrap() as i32,
        );
        b.command("app/firefox");
        let plan = b.app.native.plan().unwrap();
        assert_eq!(plan.locale.lang, "fr_FR.UTF-8");
        assert_eq!(plan.locale.keyboard.layout, "de");
        assert_eq!(plan.locale.keyboard.variant, "nodeadkeys");
        assert_eq!(plan.locale.keyboard.console, "de-latin1");
        assert_eq!(plan.locale.timezone, "Asia/Tokyo");
        assert!(plan.apps.contains("firefox"));
        assert_eq!(plan.prefer, borea_core::SourcePreference::Aur);
        let vesktop = borea_catalog::APPS.iter().find(|app| app.id == "vesktop").unwrap();
        assert!(app_available(vesktop));
        assert!(!b.app.native.ready());
        b.next();
        assert_eq!(
            b.app.native.page,
            Page::Account,
            "an unavailable review service cannot approve an installation"
        );
    }
}

#[cfg(test)]
mod iced_tests {
    use super::*;
    fn app() -> App {
        App::from_workflow(LiveInstaller::with_hardware(showcase_fixture::machine(), true), false)
    }
    fn path(a: &mut App, cachy: bool) {
        a.bridge.app.go(Step::System);
        a.bridge.command("family/0");
        a.bridge.command(if cachy { "distro/1" } else { "distro/0" });
        a.bridge.command(if cachy { "edition/2" } else { "edition/3" });
        a.bridge.next();
        a.bridge.command("mode/compose");
        a.bridge.command("compositor/hyprland");
        a.bridge.command("shell/noctalia");
        a.bridge.next();
    }
    #[test]
    fn arch_and_opensuse_are_available_and_disabled_families_cannot_change_selection() {
        let mut a = app();
        a.bridge.app.go(Step::System);
        a.bridge.next();
        assert_eq!(a.bridge.app.step, Step::System);
        let (heroes, _) = a.bridge.tiles();
        for (tile, family) in heroes.iter().zip(Family::ALL) {
            let ready = matches!(family, Family::Arch | Family::Opensuse);
            assert_eq!(tile.enabled, ready, "{family}");
            assert_eq!(tile.meta == "Not available yet", !ready, "{family}");
        }
        for family in [Family::Fedora, Family::Debian] {
            let i = Family::ALL.iter().position(|f| *f == family).unwrap();
            a.bridge.command(&format!("family/{i}"));
            a.bridge.app.apply(Action::Family(family));
            assert_eq!(a.bridge.app.step, Step::System);
            assert!(a.bridge.app.picked_family.is_none());
            assert!(a.bridge.pending_family.is_none());
        }
        a.bridge.command("family/0");
        assert_eq!(a.bridge.app.step, Step::Distribution);
        assert_eq!(a.bridge.app.picked_family, Some(Family::Arch));
    }
    #[test]
    fn tumbleweed_minimal_installs_each_compositor_with_noctalia_natively() {
        for compositor in ["hyprland", "niri", "mango"] {
            let mut a = app();
            a.bridge.app.go(Step::System);
            let i = Family::ALL.iter().position(|f| *f == Family::Opensuse).unwrap();
            a.bridge.command(&format!("family/{i}"));
            assert_eq!(a.bridge.app.step, Step::Distribution);
            a.bridge.command("distro/0");
            assert_eq!(a.bridge.app.distro().id.as_str(), "tumbleweed");
            let minimal = a.bridge.app.distro().editions.iter().position(|e| e.composable).unwrap();
            a.bridge.command(&format!("edition/{minimal}"));
            a.bridge.next();
            a.bridge.command("mode/compose");
            a.bridge.command(&format!("compositor/{compositor}"));
            a.bridge.command("shell/noctalia");
            a.bridge.next();
            assert!(a.bridge.native(), "{compositor}: Tumbleweed installs natively");
            let _ = a.update(Message::Tick(Instant::now()));
            let n = &a.bridge.app.native;
            assert_eq!(n.distro_for_test(), borea_core::Distribution::OpenSuse);
            assert_eq!(n.derivative_for_test(), None);
            a.bridge.app.native.disks = showcase_disks();
            a.bridge.command("whole/0");
            a.bridge.next();
            showcase_account(&mut a);
            let plan = a.bridge.app.native.plan().unwrap();
            assert_eq!(plan.distro, borea_core::Distribution::OpenSuse);
            assert_eq!(plan.release, "tumbleweed");
            assert_eq!(plan.prefer, borea_core::SourcePreference::Native);
            assert_eq!(
                plan.desktop.and_then(|d| d.noctalia()).map(|r| r.compositor),
                Some(compositor)
            );
            let (_, hw) = borea_catalog::hardware::samples().remove(0);
            assert!(borea_core::review(&plan, &hw).is_ok(), "{compositor}");
        }
    }

    #[test]
    fn real_iced_family_button_dispatches_a_choice() {
        use iced_runtime::user_interface::{Cache, UserInterface};
        let mut a = app();
        a.bridge.app.go(Step::System);
        let mut renderer =
            iced::Renderer::Secondary(iced_tiny_skia::Renderer::new(FACE, iced::Pixels(14.)));
        let mut ui = UserInterface::build(
            a.view(),
            iced::Size::new(1120., 753.),
            Cache::new(),
            &mut renderer,
        );
        let mut clipboard = iced::advanced::clipboard::Null;
        let cursor = iced::mouse::Cursor::Available(iced::Point::new(310., 230.));
        let mut messages = vec![];
        let events = [
            iced::Event::Mouse(iced::mouse::Event::ButtonPressed(iced::mouse::Button::Left)),
            iced::Event::Mouse(iced::mouse::Event::ButtonReleased(iced::mouse::Button::Left)),
        ];
        let _ = ui.update(&events, cursor, &mut renderer, &mut clipboard, &mut messages);
        drop(ui);
        assert!(messages.iter().any(|m| matches!(m,Message::Choose(k)if k=="family/0")));
        for m in messages {
            let _ = a.update(m);
        }
        assert_eq!(a.bridge.app.step, Step::Distribution);
    }
    #[test]
    fn captures_draw_their_words() {
        load_fonts().unwrap();
        let a = app();
        let path = std::env::temp_dir().join(format!("borea-words-{}.png", std::process::id()));
        capture(&a, &path, 1120, 753).unwrap();
        let shot = ::image::open(&path).unwrap().to_luma8();
        let _ = std::fs::remove_file(&path);
        // "Borea" at the top of the rail is light ice on a dark rail.
        let brightest = (20..200)
            .flat_map(|x| (20..60).map(move |y| (x, y)))
            .map(|(x, y)| shot.get_pixel(x, y)[0])
            .max()
            .unwrap();
        assert!(brightest > 120, "the rail's title was not drawn");
    }
    #[test]
    fn arch_and_cachyos_use_distinct_reviewed_targets() {
        for cachy in [false, true] {
            let mut a = app();
            path(&mut a, cachy);
            assert!(a.bridge.native());
            assert_eq!(
                a.bridge.app.native.derivative_for_test(),
                cachy.then_some(borea_core::ArchDerivative::Cachyos)
            );
            assert!(!a.nav().1, "no disk is preselected");
        }
    }
    #[test]
    fn selecting_another_target_clears_storage_and_approval() {
        let mut a = app();
        path(&mut a, false);
        a.bridge.app.native.pick = Some(Where::Whole { disk: 0 });
        a.bridge.app.go(Step::Distribution);
        a.bridge.command("distro/1");
        a.bridge.command("edition/2");
        a.bridge.next();
        a.bridge.command("mode/compose");
        a.bridge.command("compositor/hyprland");
        a.bridge.command("shell/noctalia");
        a.bridge.next();
        assert!(a.bridge.app.native.pick.is_none());
        assert!(!a.bridge.app.native.ready());
    }
    #[test]
    fn wifi_join_rejects_short_or_unsupported_keys() {
        let mut a = app();
        a.hidden = true;
        a.hidden_ssid = "Example".into();
        *a.wifi_password = "short".into();
        assert!(!a.join_valid());
        *a.wifi_password = "eight-or-more".into();
        assert!(a.join_valid());
        a.hidden_security = "Open".into();
        *a.wifi_password = String::new();
        assert!(a.join_valid());
        a.hidden_ssid = String::new();
        assert!(!a.join_valid());
    }
    #[test]
    fn credentials_are_redacted_and_erased_outside_the_account_page() {
        let mut a = app();
        path(&mut a, false);
        a.bridge.app.native.page = Page::Account;
        let secret = "never-print-this";
        let message = Message::Field("password", secret.into());
        assert!(!format!("{message:?}").contains(secret));
        let _ = a.update(message);
        assert_eq!(a.password.as_str(), secret);
        a.bridge.app.native.page = Page::Review;
        let _ = a.update(Message::Tick(Instant::now()));
        assert!(a.password.is_empty());
    }
    #[test]
    fn firmware_confirmation_blocks_page_mutation_and_preview_restart() {
        let mut a = app();
        let _ = a.update(Message::Action(Action::OpenFirmware));
        let step = a.bridge.app.step;
        let _ = a.update(Message::Next);
        assert_eq!(a.bridge.app.step, step);
        let _ = a.update(Message::Action(Action::RestartFirmware));
        assert!(!a.bridge.app.firmware.restarting);
        assert!(a.bridge.app.firmware.error.as_deref().is_some_and(|e| e.contains("preview")));
    }
    #[test]
    fn boot_smoke_exits_only_after_the_event_loop_ticks() {
        let mut a = app();
        a.boot_smoke = true;
        assert!(!a.ready);
        let _ = a.update(Message::Tick(Instant::now()));
        assert!(a.ready);
    }
    #[test]
    fn paused_motion_does_not_advance_card_effects() {
        let mut a = app();
        let elapsed = a.elapsed;
        let pulse = a.pulse;
        let _ = a.update(Message::Tick(Instant::now() + Duration::from_secs(2)));
        assert_eq!(a.elapsed, elapsed);
        assert_eq!(a.pulse, pulse);
    }
    #[test]
    fn family_cards_lift_under_the_pointer_and_settle_back() {
        let mut a = app();
        a.bridge.app.still = false;
        a.bridge.app.go(Step::System);
        let _ = a.update(Message::Hover("family/0".into(), true));
        let _ = a.update(Message::Pointer(iced::Point::new(10., 10.)));
        a.ease_cards(0.03, false);
        let first = a.lift["family/0"];
        assert!(first > 0. && first < 0.5, "eases in, never jumps: {first}");
        for _ in 0..20 {
            a.ease_cards(0.03, false);
        }
        assert!(a.lift["family/0"] > 0.99);
        assert_eq!(a.lift.len(), 1, "only the hovered card lifts");
        let _ = a.update(Message::Hover("family/0".into(), false));
        assert!(a.hovered.is_none() && a.pointer.is_none());
        for _ in 0..40 {
            a.ease_cards(0.03, false);
        }
        assert!(a.lift.is_empty(), "settled cards drop out");
    }
    #[test]
    fn a_picked_family_washes_in_before_the_page_moves_on() {
        let mut a = app();
        a.bridge.app.still = false;
        a.bridge.app.go(Step::System);
        let _ = a.update(Message::Choose("family/0".into()));
        assert!(a.bridge.pending_family.is_some());
        // The page moves on after 280 ms; by then the wash is nearly all in.
        for _ in 0..9 {
            a.ease_cards(0.03, false);
        }
        assert!(a.chosen["family/0"] > 0.9, "{}", a.chosen["family/0"]);
        assert!(a.chosen.keys().all(|k| k == "family/0" || k.starts_with("step/")));
    }
    #[test]
    fn paused_family_cards_snap_and_leaving_the_step_drops_the_hover() {
        let mut a = app();
        a.bridge.app.go(Step::System);
        let _ = a.update(Message::Hover("family/0".into(), true));
        a.ease_cards(0., true);
        assert_eq!(a.lift["family/0"], 1., "paused, the state still shows, without motion");
        a.bridge.app.go(Step::Distribution);
        let _ = a.update(Message::Tick(Instant::now()));
        assert!(a.hovered.is_none() && a.pointer.is_none());
        assert!(a.lift.is_empty());
    }
    #[test]
    fn distribution_cards_speak_like_family_cards() {
        let mut a = app();
        a.bridge.app.apply(Action::Family(Family::Arch));
        let (heroes, _) = a.bridge.tiles();
        assert!(!heroes.is_empty());
        for t in &heroes {
            assert!(crate::model::distro_quote(&t.logo).is_some(), "{}: its own words", t.logo);
            assert!(t.badge.is_some(), "{}: a status tag", t.logo);
        }
        assert_eq!(
            heroes[0].meta,
            crate::model::family_release(Family::Arch),
            "the flagship says what its family card says"
        );
    }
    #[test]
    fn the_rail_eases_like_any_other_card() {
        let mut a = app();
        a.bridge.app.still = false;
        a.ease_cards(0.03, false);
        let key = step_key(a.bridge.app.step);
        assert!(a.chosen.get(&key).is_some_and(|c| *c > 0. && *c < 1.), "{:?}", a.chosen);
    }
    #[test]
    fn every_family_has_its_own_words_colour_and_release() {
        use crate::model::{family_colour, family_quote, family_release};
        for f in Family::ALL {
            let q = family_quote(f);
            assert!(!q.text.is_empty() && !q.source.is_empty(), "{f}");
            assert!(!family_release(f).is_empty(), "{f}");
        }
        let colours: std::collections::HashSet<_> =
            Family::ALL.into_iter().map(family_colour).collect();
        assert_eq!(colours.len(), Family::ALL.len());
    }
    #[test]
    fn blank_account_has_no_implicit_region_or_credentials() {
        let mut a = app();
        path(&mut a, false);
        a.bridge.app.native.page = Page::Account;
        let n = &a.bridge.app.native;
        assert!(n.user.username.is_empty());
        assert!(n.locale.lang.is_empty());
        assert!(n.locale.timezone.is_empty());
        assert!(n.locale.keyboard.layout.is_empty());
        assert!(n.apps.is_empty());
    }
    #[test]
    fn live_and_mounted_disks_are_never_offered() {
        let disks: Vec<handoff::Disk>=serde_json::from_value(serde_json::json!([
            {"id":{"logical_sector":512,"path":"/dev/sda","serial":"STICK","size":64000000000u64,"model":"Live USB"},"readonly":false,"in_use":true,"live":true,"parts":[]},
            {"id":{"logical_sector":512,"path":"/dev/vda","serial":"TEST","size":100000000000u64,"model":"Target"},"readonly":false,"in_use":false,"live":false,"parts":[]}
        ])).unwrap();
        let groups = storage_tiles(&disks, &None, 40);
        assert!(groups[0].choices.iter().all(|t| !t.label.contains("Live USB")));
        assert!(groups[0].choices.iter().any(|t| t.key == "whole/1"));
    }
    #[test]
    fn alongside_choice_is_bounded_and_requires_ntfs() {
        let mut a = app();
        path(&mut a, false);
        a.bridge.app.native.disks=serde_json::from_value(serde_json::json!([{
            "id":{"logical_sector":512,"path":"/dev/vda","serial":"TEST","size":200000000000u64,"model":"Target"},"readonly":false,"in_use":false,"live":false,
            "parts":[{"path":"/dev/vda3","number":3,"uuid":"WIN","fs":"ntfs","max_linux_size":80*handoff::GIB}]}])).unwrap();
        a.bridge.command("alongside/0/0");
        a.bridge.space(200.);
        assert!(
            matches!(a.bridge.app.native.pick,Some(Where::Alongside{size,..})if size==80*handoff::GIB)
        );
        a.bridge.space(1.);
        assert!(
            matches!(a.bridge.app.native.pick,Some(Where::Alongside{size,..})if size==40*handoff::GIB)
        );
        a.bridge.app.native.pick = None;
        a.bridge.app.native.disks[0].parts[0].fs = "ext4".into();
        a.bridge.command("alongside/0/0");
        assert!(a.bridge.app.native.pick.is_none());
    }
    #[test]
    fn extras_are_chosen_only_on_the_review() {
        let mut a = app();
        a.bridge.app.go(Step::System);
        a.bridge.command("family/0");
        a.bridge.command("distro/2");
        a.bridge.command("edition/0");
        a.bridge.next();
        // The Desktop step offers no extras.
        if a.bridge.app.step == Step::Desktop {
            let (_, groups) = a.bridge.tiles();
            assert!(groups.iter().flat_map(|g| &g.choices).all(|t| !t.key.starts_with("extra/")));
            a.bridge.next();
        }
        assert_eq!(a.bridge.app.step, Step::Review);
        a.bridge.app.handoff.phase = Phase::Choosing;
        let (_, groups) = a.bridge.tiles();
        let extras: Vec<_> = groups
            .iter()
            .flat_map(|g| &g.choices)
            .filter(|t| t.key.starts_with("extra/"))
            .collect();
        assert!(extras.iter().any(|t| t.key == "extra/gaming"), "the review offers them");
    }
    #[test]
    fn an_app_an_extra_installs_shows_as_included_not_twice() {
        let mut a = app();
        a.bridge.app.go(Step::System);
        a.bridge.command("family/0");
        a.bridge.command("distro/0");
        let steam = borea_catalog::APPS.iter().find(|x| x.id == "steam").unwrap();
        assert_eq!(a.included_by(steam), None);
        a.bridge.command("extra/gaming");
        assert_eq!(a.included_by(steam).as_deref(), Some("Gaming"));
        let firefox = borea_catalog::APPS.iter().find(|x| x.id == "firefox").unwrap();
        assert_eq!(a.included_by(firefox), None);
    }
    #[test]
    fn omarchy_reaches_its_upstream_handoff_and_keeps_extras() {
        let mut a = app();
        a.bridge.app.go(Step::System);
        a.bridge.command("family/0");
        a.bridge.command("distro/2");
        a.bridge.command("edition/0");
        a.bridge.next();
        a.bridge.command("extra/printing");
        a.bridge.next();
        assert_eq!(a.bridge.app.handoff_target(), Some("omarchy"));
        assert!(!a.bridge.native());
        assert!(a.bridge.app.handoff.extras.contains(&"printing".into()));
    }
    #[test]
    fn native_services_and_hardware_poll_without_gui_contexts() {
        let mut a = app();
        let _ = a.update(Message::Tick(Instant::now()));
        assert!(a.bridge.app.hardware().is_some());
    }
}

fn input_style(p: Colors, status: text_input::Status) -> text_input::Style {
    text_input::Style {
        background: p.surface.into(),
        border: Border {
            color: if matches!(
                status,
                text_input::Status::Focused { .. } | text_input::Status::Hovered
            ) {
                p.ice
            } else {
                p.line
            },
            width: 1.,
            radius: 10.into(),
        },
        icon: p.muted,
        placeholder: p.muted,
        value: p.text,
        selection: p.selected,
    }
}

#[cfg(test)]
mod network_tests {
    use super::*;
    #[test]
    fn rescans_keep_the_selected_ssid_and_erase_a_lost_networks_key() {
        let mut app = App::from_workflow(
            LiveInstaller::with_hardware(showcase_fixture::machine(), true),
            false,
        );
        let network = |ssid| {
            borea_network::Network::sample(
                ssid,
                80,
                borea_network::Security::WpaPersonal,
                borea_network::Band::Ghz5,
                false,
            )
        };
        app.bridge.app.wifi.networks = vec![network("Chosen"), network("Other")];
        app.bridge.command("network/0");
        *app.wifi_password = "never-send-to-another-network".into();
        app.bridge.app.wifi.networks.swap(0, 1);
        let _ = app.update(Message::Tick(Instant::now()));
        assert_eq!(app.bridge.network, Some(1));
        assert!(!app.wifi_password.is_empty());
        app.bridge.app.wifi.networks.remove(1);
        let _ = app.update(Message::Tick(Instant::now()));
        assert!(app.bridge.network.is_none());
        assert!(app.wifi_password.is_empty());
    }
    #[test]
    fn disk_encryption_is_offered_only_in_setup_mode() {
        for setup in [false, true] {
            let mut hardware = showcase_fixture::machine();
            hardware.setup_mode = Fact::Known(setup);
            let mut app = App::from_workflow(LiveInstaller::with_hardware(hardware, true), false);
            let b = &mut app.bridge;
            b.app.go(Step::System);
            b.command("family/0");
            b.command("distro/0");
            b.command("edition/3");
            b.next();
            b.command("mode/compose");
            b.command("compositor/hyprland");
            b.command("shell/noctalia");
            b.next();
            b.app.native.disks = showcase_disks();
            b.command("whole/0");
            b.next();
            assert_eq!(b.app.native.page, Page::Account);
            let _ = app.update(Message::Encrypt(true));
            assert_eq!(app.bridge.app.native.encrypt, setup);
        }
    }
    #[test]
    fn native_secure_boot_requires_firmware_setup_mode() {
        let mut hardware = showcase_fixture::machine();
        hardware.setup_mode = Fact::Known(false);
        let mut bridge = Bridge {
            app: LiveInstaller::with_hardware(hardware, true),
            network: None,
            network_name: None,
            pending_family: None,
        };
        bridge.app.go(Step::System);
        bridge.command("family/0");
        bridge.command("distro/0");
        bridge.command("edition/3");
        bridge.next();
        bridge.command("mode/compose");
        bridge.command("compositor/hyprland");
        bridge.command("shell/noctalia");
        assert!(matches!(bridge.app.secure_boot(), Some(Readiness::Unavailable(_))));
        bridge.app.apply(Action::SecureBoot(true));
        assert!(!bridge.app.selection.secure_boot);
    }
}

#[cfg(test)]
mod completion_tests {
    use super::*;
    #[test]
    fn completed_installation_keeps_target_locked_and_firmware_available() {
        let mut a = App::from_workflow(
            LiveInstaller::with_hardware(showcase_fixture::machine(), true),
            false,
        );
        a.bridge.app.go(Step::System);
        a.bridge.command("family/0");
        a.bridge.command("distro/0");
        a.bridge.command("edition/3");
        a.bridge.next();
        a.bridge.command("mode/compose");
        a.bridge.command("compositor/hyprland");
        a.bridge.command("shell/noctalia");
        a.bridge.next();
        a.bridge.app.native.page = Page::Finished;
        a.bridge.app.native.progress = 1.;
        a.bridge.back();
        a.bridge.command("family/2");
        a.bridge.app.apply(Action::Go(Step::System));
        assert_eq!(a.bridge.app.step, Step::Review);
        assert_eq!(a.bridge.app.family(), Family::Arch);
        assert_eq!(a.bridge.app.install_progress(), Some(1.));
        a.bridge.app.apply(Action::OpenFirmware);
        assert!(a.bridge.app.firmware_dialog);
    }
    #[test]
    fn changing_edition_requires_a_new_disk_choice() {
        let mut a = App::from_workflow(
            LiveInstaller::with_hardware(showcase_fixture::machine(), true),
            false,
        );
        a.bridge.app.go(Step::System);
        a.bridge.command("family/0");
        a.bridge.command("distro/0");
        a.bridge.command("edition/3");
        a.bridge.app.native.pick = Some(Where::Whole { disk: 0 });
        a.bridge.app.native.page = Page::Account;
        a.bridge.command("edition/3");
        assert!(a.bridge.app.native.pick.is_none());
        assert_eq!(a.bridge.app.native.page, Page::Storage);
    }
}

#[cfg(test)]
mod compositor_tests {
    use super::*;
    #[test]
    fn all_three_noctalia_compositors_reach_the_correct_native_plan() {
        for (id, desktop) in [
            ("hyprland", borea_core::Desktop::HyprlandNoctalia),
            ("niri", borea_core::Desktop::NiriNoctalia),
            ("mango", borea_core::Desktop::MangoNoctalia),
        ] {
            for cachyos in [false, true] {
                let mut b = Bridge {
                    app: LiveInstaller::with_hardware(showcase_fixture::machine(), true),
                    network: None,
                    network_name: None,
                    pending_family: None,
                };
                b.app.go(Step::System);
                b.command("family/0");
                b.command(if cachyos { "distro/1" } else { "distro/0" });
                b.command(if cachyos { "edition/2" } else { "edition/3" });
                b.next();
                b.command("mode/compose");
                b.command(&format!("compositor/{id}"));
                b.command("shell/noctalia");
                b.next();
                assert!(b.native());
                assert_eq!(b.app.native_desktop(), Some(desktop));
                assert_eq!(b.app.native.page, Page::Storage);
                assert!(b.app.native.pick.is_none());
                b.app.native.disks = serde_json::from_value(serde_json::json!([{
                    "id": {"logical_sector":512,"path":"/dev/vda","serial":"TEST",
                        "size":107374182400u64,"model":"Disposable VM disk"},
                    "parts": []
                }]))
                .unwrap();
                b.command("whole/0");
                b.next();
                for (field, value) in
                    [("full-name", "Test Person"), ("username", "tester"), ("hostname", "test-pc")]
                {
                    b.app.native.edit_account(field, value.into());
                }
                b.region(
                    "language",
                    borea_locale::LOCALES.iter().position(|v| v.id == "fr_FR.UTF-8").unwrap()
                        as i32,
                );
                b.region(
                    "keyboard",
                    borea_locale::KEYBOARDS
                        .iter()
                        .position(|v| v.layout == "de" && v.variant == "nodeadkeys")
                        .unwrap() as i32,
                );
                b.region(
                    "console",
                    borea_locale::console_keymaps().position(|v| v == "de-latin1").unwrap() as i32,
                );
                b.region(
                    "timezone",
                    borea_locale::TIMEZONES.iter().position(|v| *v == "Europe/Paris").unwrap()
                        as i32,
                );
                let plan = b.app.native.plan().unwrap();
                assert_eq!(plan.locale.lang, "fr_FR.UTF-8");
                assert_eq!(plan.locale.keyboard.variant, "nodeadkeys");
                assert_eq!(plan.desktop, Some(desktop));
                assert_eq!(plan.derivative, cachyos.then_some(borea_core::ArchDerivative::Cachyos));
                let encoded = serde_json::to_value(&plan).unwrap();
                assert_eq!(encoded["desktop"], desktop.key());
                let decoded: borea_core::Plan = serde_json::from_value(encoded).unwrap();
                assert_eq!(decoded, plan);
            }
        }
    }
}
