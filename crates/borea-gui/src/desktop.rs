//! Iced presentation. The installer workflow and workers contain no GUI types.
use super::*;
use crate::install::Page;
use crate::model::hardware_facts;
use iced::widget::{
    self, button, checkbox, column, container, image, pick_list, progress_bar, row, scrollable,
    slider, space, stack, svg, text, text_input,
};
use iced::{Alignment, Background, Border, Color, Element, Length, Subscription, Task, Theme};
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
            "family" => index(1).and_then(|i| Family::ALL.get(i).copied()).and_then(|f| {
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
                }
            }
            "keyboard" => {
                if let Some(v) = borea_locale::KEYBOARDS.get(i) {
                    n.locale.keyboard.layout = v.layout.into();
                    n.locale.keyboard.variant = v.variant.into();
                }
            }
            "console" => {
                if let Some(v) = borea_locale::console_keymaps().nth(i) {
                    n.locale.keyboard.console = v.into();
                }
            }
            "timezone" => {
                if let Some(v) = borea_locale::TIMEZONES.get(i) {
                    n.locale.timezone = (*v).into();
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
                        self.pending_family.map(|(picked, _)| picked).or(a.picked_family)
                            == Some(f),
                    );
                    t.logo = family_id(f).into();
                    t.meta = match f {
                        Family::Arch => "Rolling release",
                        Family::Fedora => "Regular releases",
                        Family::Debian => "Stable release",
                        Family::Opensuse => "Rolling snapshots",
                    }
                    .into();
                    heroes.push(t);
                }
            }
            Step::Distribution => {
                for (i, d) in a.catalog.family(f).enumerate() {
                    let status = if d.id.as_str() == "omarchy" {
                        "Official installer, via Borea".into()
                    } else if matches!(d.id.as_str(), "arch" | "cachyos") {
                        "Minimal · Hyprland + Noctalia".into()
                    } else {
                        format!("{} integration", crate::model::capitalize(&d.status.to_string()))
                    };
                    let mut t = Tile::new(
                        format!("distro/{i}"),
                        &d.name,
                        format!("{}\n\n{status}", d.summary),
                        a.picked_distro && c.distro == i,
                    );
                    t.logo = d.id.to_string();
                    t.meta = crate::model::capitalize(&d.channel.to_string());
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
                                "Console login and your selected extras",
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
                                    t
                                })
                                .collect();
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
                        vec![Tile::new(
                            "",
                            &a.edition().name,
                            "The upstream desktop is included",
                            true,
                        )],
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
                groups.push(section(
                    "Extras",
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
                            t
                        })
                        .collect(),
                ));
            }
            Step::Review if self.native() => match a.native.page {
                Page::Storage => groups.extend(storage_tiles(&a.native.disks, &a.native.pick, 40)),
                Page::Account => {
                    let sources = borea_catalog::Sources::defaults(borea_catalog::Distro::Arch);
                    groups.push(section(
                        "Extra applications · installed quietly after login",
                        borea_catalog::APPS
                            .iter()
                            .map(|app| {
                                let mut t = Tile::new(
                                    format!("app/{}", app.id),
                                    app.name,
                                    "",
                                    a.native.apps.contains(app.id),
                                );
                                t.enabled = matches!(
                                    borea_catalog::resolve(
                                        app,
                                        &sources,
                                        borea_catalog::Prefer::Native
                                    ),
                                    borea_catalog::Resolution::From {
                                        source: borea_catalog::Source::Repo(_)
                                            | borea_catalog::Source::Flathub,
                                        ..
                                    }
                                );
                                t
                            })
                            .collect(),
                    ));
                }
                _ => {}
            },
            Step::Review if a.handoff_target().is_some() => {
                if matches!(
                    a.handoff.phase,
                    Phase::Choosing | Phase::Reviewing | Phase::Reviewed(_)
                ) {
                    groups.extend(storage_tiles(&a.handoff.disks, &a.handoff.pick, 32));
                }
            }
            _ => {}
        }
        (heroes, groups)
    }
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
    WifiPassword(String),
    Reveal(bool),
    Join,
    Rescan,
    Hidden(bool),
    HiddenSsid(String),
    HiddenSecurity(String),
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
            Self::Reveal(..) => "Reveal",
            Self::Join => "Join",
            Self::Rescan => "Rescan",
            Self::Hidden(..) => "Hidden",
            Self::HiddenSecurity(..) => "HiddenSecurity",
        })
    }
}
const FONT: &[u8] = include_bytes!("../assets/NotoSans-Regular.ttf");
const FACE: iced::Font = iced::Font::with_name("Noto Sans");
struct App {
    bridge: Bridge,
    wind: crate::frost_motion::FrostMotion,
    last: Instant,
    view_key: String,
    elapsed: f32,
    pulse: f32,
    password: zeroize::Zeroizing<String>,
    confirmation: zeroize::Zeroizing<String>,
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
            password: Default::default(),
            confirmation: Default::default(),
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
                if self.bridge.app.native.page != Page::Account {
                    use zeroize::Zeroize;
                    self.password.zeroize();
                    self.confirmation.zeroize();
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
            Message::Back => self.bridge.back(),
            Message::Next => self.bridge.next(),
            Message::Choose(key) => {
                let prior = self.bridge.network;
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
                    _ => {}
                }
                self.bridge.app.native.edit_account(field, value);
            }
            Message::Region(kind, value)
                if !self.bridge.busy() && !self.bridge.app.firmware_dialog =>
            {
                let index = region_options(kind).iter().position(|s| s == &value);
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
            iced::time::every(if self.bridge.app.still {
                Duration::from_millis(150)
            } else {
                crate::wind::FRAME
            })
            .map(Message::Tick),
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
            space().height(28)
        ]
        .spacing(9);
        for step in Step::ALL {
            let active = step == a.step;
            let mut content =
                column![text(format!("{:02}  {}", step.number(), step.name())).size(14)].spacing(5);
            if let Some(summary) = a.summary(step) {
                content = content.push(text(summary).size(11).color(p.muted));
            }
            let enabled =
                step <= a.reached && !self.bridge.busy() && a.native.page != Page::Finished;
            rail = rail.push(
                button(content)
                    .padding(12)
                    .width(Length::Fill)
                    .on_press_maybe(enabled.then_some(Message::Action(Action::Go(step))))
                    .style(move |_, status| tile_style(p, active, status)),
            );
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
            text(format!("STEP {:02} / 06", a.step.number())).size(11).color(p.ice),
            text(self.heading()).size(31),
            text(a.step.lead()).size(13).color(p.muted)
        ]
        .spacing(12);
        if !heroes.is_empty() {
            let mut cards = row![].spacing(12);
            for tile in heroes {
                cards = cards.push(self.card(tile, true));
            }
            body = body.push(cards);
        }
        if self.bridge.native() && a.native.page == Page::Account {
            body = body.push(self.account());
        }
        if a.step == Step::Connect {
            body =
                body.push(button("Rescan networks").on_press(Message::Rescan).style(button::text));
        }
        for group in groups {
            if self.bridge.native() && a.native.page == Page::Account && !self.apps_open {
                continue;
            }
            body = body.push(text(group.title).size(12).color(p.ice));
            // Compact rows keep long shell/app catalogs readable at laptop resolutions.
            for chunk in group
                .choices
                .chunks(if a.step == Step::Connect || a.step == Step::Review { 1 } else { 2 })
            {
                let mut choices = row![].spacing(10);
                for tile in chunk {
                    choices = choices.push(self.card(tile.clone(), false));
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
        let main =
            column![scrollable(body.padding([8, 4])).height(Length::Fill), footer].spacing(18);
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
                    border: Border { color: p.ice, width: 1., radius: 20.into() },
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
    fn card(&self, tile: Tile, hero: bool) -> Element<'_, Message> {
        let p = self.colors();
        let selected = tile.selected;
        let mut title = column![].spacing(if hero { 14 } else { 6 });
        if hero {
            if let Some(logo) = logo(&tile.logo, self.bridge.app.dark) {
                title = title.push(logo);
            }
            title = title.push(text(tile.meta).size(10).color(p.ice));
        }
        title = title
            .push(text(tile.label).size(if hero { 19 } else { 15 }))
            .push(text(tile.description).size(12).color(p.muted));
        if selected {
            title = title.push(text("SELECTED").size(9).color(p.ice));
        }
        let enabled = tile.enabled && !tile.key.is_empty() && !self.bridge.busy();
        let pick = Message::Choose(tile.key);
        let card: Element<'_, Message> = button(title)
            .padding(if hero { 22 } else { 14 })
            .width(Length::Fill)
            .height(if hero { Length::Fixed(255.) } else { Length::Shrink })
            .on_press_maybe(enabled.then_some(pick))
            .style(move |_, status| tile_style(p, selected, status))
            .into();
        if selected {
            let energy =
                widget::canvas(SelectionFx { t: self.elapsed, pulse: self.pulse, color: p.ice })
                    .width(Length::Fill)
                    .height(Length::Fill);
            stack![card, energy].into()
        } else {
            card
        }
    }
    fn account(&self) -> Element<'_, Message> {
        let n = &self.bridge.app.native;
        let p = self.colors();
        let field = |label: &'static str,
                     key: &'static str,
                     value: &str,
                     secret: bool|
         -> Element<'_, Message> {
            column![
                text(label).size(12),
                text_input(label, value)
                    .secure(secret)
                    .on_input(move |s| Message::Field(key, s))
                    .padding(12)
                    .style(move |_, status| input_style(p, status))
            ]
            .spacing(6)
            .width(Length::Fill)
            .into()
        };
        let mut content = column![
            row![
                field("Full name", "full-name", &n.user.full_name, false),
                field("Username", "username", &n.user.username, false)
            ]
            .spacing(12),
            field("Computer name", "hostname", &n.user.hostname, false),
            row![
                field("Password", "password", &self.password, true),
                field("Confirm password", "confirmation", &self.confirmation, true)
            ]
            .spacing(12),
        ]
        .spacing(14)
        .width(Length::Fill);
        for (kind, label) in [
            ("language", "Language"),
            ("keyboard", "Keyboard"),
            ("console", "Disk unlock keyboard"),
            ("timezone", "Time zone"),
        ] {
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
            content = content.push(
                column![
                    text(label).size(12),
                    pick_list(options, selected, move |s| Message::Region(kind, s))
                        .placeholder("Choose…")
                        .width(Length::Fill)
                        .padding(12)
                        .style(move |_, _| pick_list::Style {
                            text_color: p.text,
                            placeholder_color: p.muted,
                            handle_color: p.ice,
                            background: p.surface.into(),
                            border: Border { color: p.line, width: 1., radius: 10.into() },
                        })
                ]
                .spacing(6)
                .width(Length::Fill),
            );
        }
        content
            .push(
                checkbox(self.apps_open)
                    .label("Choose extra applications (optional)")
                    .on_toggle(Message::Apps),
            )
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
            if let Some(e) = &n.error {
                body = body.push(text(e.clone()).color(Color::from_rgb8(208, 135, 112)));
            }
            match n.page {
                Page::Storage if n.disks.is_empty() => {
                    body = body.push(text("Looking for available drives…").size(14))
                }
                Page::Review => {
                    body = body.push(text(n.review_details().unwrap_or_default()).size(14)).push(
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
                    body=body.push(text(format!("{}\n\n{}",review.steps.join("\n"),review.choose)))
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
                    "This combination is a preview. Its installation pathway is not connected yet.",
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
    raised: Color,
    text: Color,
    muted: Color,
    ice: Color,
    line: Color,
    selected: Color,
}
impl Colors {
    fn new(dark: bool) -> Self {
        let rgb = Color::from_rgb8;
        if dark {
            Self {
                bg: rgb(35, 42, 54),
                surface: rgb(46, 52, 64),
                raised: rgb(59, 66, 82),
                text: rgb(236, 239, 244),
                muted: rgb(180, 193, 209),
                ice: rgb(136, 192, 208),
                line: rgb(76, 86, 106),
                selected: rgb(48, 76, 94),
            }
        } else {
            Self {
                bg: rgb(236, 239, 244),
                surface: rgb(229, 233, 240),
                raised: rgb(216, 222, 233),
                text: rgb(46, 52, 64),
                muted: rgb(76, 86, 106),
                ice: rgb(46, 99, 126),
                line: rgb(170, 187, 203),
                selected: rgb(206, 227, 237),
            }
        }
    }
}
fn tile_style(p: Colors, selected: bool, status: button::Status) -> button::Style {
    let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
    button::Style {
        background: Some(Background::Color(if selected {
            p.selected
        } else if hovered {
            p.raised
        } else {
            p.surface
        })),
        text_color: if matches!(status, button::Status::Disabled) && !selected {
            p.muted
        } else {
            p.text
        },
        border: Border {
            color: if selected || hovered { p.ice } else { p.line },
            width: if selected { 2. } else { 1. },
            radius: 16.into(),
        },
        shadow: iced::Shadow {
            color: Color { a: if selected { 0.17 } else { 0. }, ..p.ice },
            offset: iced::Vector::ZERO,
            blur_radius: 18.,
        },
        ..Default::default()
    }
}
fn logo(id: &str, dark: bool) -> Option<Element<'static, Message>> {
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
            return Some(
                image(image::Handle::from_bytes(
                    include_bytes!("../ui/logos/fedora.png").as_slice(),
                ))
                .width(52)
                .height(52)
                .into(),
            );
        }
        _ => return None,
    };
    let mark = svg(svg::Handle::from_memory(bytes))
        .width(if id == "omarchy" { 96 } else { 52 })
        .height(52);
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
struct SelectionFx {
    t: f32,
    pulse: f32,
    color: Color,
}
impl widget::canvas::Program<Message> for SelectionFx {
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
        let w = bounds.width;
        let h = bounds.height;
        // Organic icy veins hug the selected edge; they never frame the logo.
        for edge in 0..2 {
            let y = if edge == 0 { 4. } else { h - 4. };
            let path = Path::new(|b| {
                b.move_to(iced::Point::new(18., y));
                for i in 1..=32 {
                    let x = 18. + (w - 36.) * i as f32 / 32.;
                    let wave = (i as f32 * 0.7 + self.t * 1.1).sin() * 1.4;
                    b.line_to(iced::Point::new(x, y + wave));
                }
            });
            f.stroke(
                &path,
                Stroke::default()
                    .with_width(0.7)
                    .with_color(Color { a: 0.25 + self.pulse * 0.4, ..self.color }),
            );
        }
        if self.pulse > 0. {
            let x = (1. - self.pulse) * (w + 80.) - 40.;
            for i in 0..12 {
                f.fill_rectangle(
                    iced::Point::new(x - i as f32 * 3., 6.),
                    iced::Size::new(3., (h - 12.).max(0.)),
                    Color { a: self.pulse * (1. - i as f32 / 12.) * 0.025, ..self.color },
                );
            }
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
    iced::application(move || App::new(live, boot_smoke), App::update, App::view)
        .title(if live { "Borea Live" } else { "Windpad" })
        .theme(App::theme)
        .subscription(App::subscription)
        .font(FONT)
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
    use iced_runtime::user_interface::{Cache, UserInterface};
    let mut renderer = iced_tiny_skia::Renderer::new(FACE, iced::Pixels(14.));
    let size = iced::Size::new(width as f32, height as f32);
    let mut ui = UserInterface::build(app.view(), size, Cache::new(), &mut renderer);
    ui.draw(
        &mut renderer,
        &app.theme(),
        &iced::advanced::renderer::Style { text_color: app.colors().text },
        iced::mouse::Cursor::Unavailable,
    );
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
fn showcase_account(app: &mut App) {
    for (field, value) in [
        ("full-name", "Alex"),
        ("username", "alex"),
        ("hostname", "frost"),
        ("password", "showcase-only-password"),
        ("confirmation", "showcase-only-password"),
    ] {
        let _ = app.update(Message::Field(field, value.into()));
    }
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
fn showcase(dir: &std::path::Path, animate: bool) -> Result<(), Box<dyn std::error::Error>> {
    use std::borrow::Cow;
    iced_tiny_skia::graphics::text::font_system()
        .write()
        .map_err(|_| "Font system lock failed")?
        .load_font(Cow::Borrowed(FONT));
    std::fs::create_dir_all(dir.join("motion"))?;
    let make = || {
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
        let mut workflow = LiveInstaller::with_link(showcase_fixture::machine(), wifi, true);
        // Capture fixtures cannot start installation, reboot or contact any service.
        workflow.set_install_service(std::sync::Arc::new(|method,_|match method {
            "plan.review"=>Ok(serde_json::json!({"approval":"showcase-only","review":{"packages":[{"name":"hyprland"},{"name":"noctalia"},{"name":"noctalia-greeter"}],"warnings":["Example plan. This capture does not install a system."]}})),
            _=>Err("Unavailable in showcase mode".into()),
        }));
        let mut app = App::from_workflow(workflow, false);
        app.bridge.network = Some(0);
        app
    };
    let mut app = make();
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
    app.bridge.command("family/0");
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
    capture(&app, &dir.join("account.png"), 1120, 753)?;
    capture(&app, &dir.join("account-small.png"), 900, 600)?;
    showcase_account(&mut app);
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
    app = make();
    app.bridge.app.still = false;
    let mut previous = String::new();
    for frame in 0..216 {
        match frame {
            8 => *app.wifi_password = "showcase-only-password".into(),
            20 => app.bridge.app.go(Step::Machine),
            36 => app.bridge.app.go(Step::System),
            52 => {
                app.bridge.pending_family = Some((Family::Arch, Instant::now()));
                app.pulse = 1.;
            }
            56 => {
                app.bridge.pending_family = None;
                app.bridge.app.apply(Action::Family(Family::Arch));
            }
            66 => {
                app.bridge.command("distro/0");
                app.pulse = 1.;
            }
            80 => app.bridge.command("edition/3"),
            92 => app.bridge.next(),
            100 => app.bridge.command("mode/compose"),
            110 => app.bridge.command("compositor/hyprland"),
            126 => {
                app.bridge.command("shell/noctalia");
                app.pulse = 1.;
            }
            142 => app.bridge.command("extra/gaming"),
            154 => {
                app.bridge.next();
                app.bridge.app.native.disks = showcase_disks();
            }
            168 => {
                app.bridge.command("whole/0");
                app.bridge.next();
            }
            180 => showcase_account(&mut app),
            196 => app.bridge.app.native.review(),
            _ => {}
        }
        if frame >= 196 {
            app.bridge.app.native.poll();
        }
        let next = app.key();
        app.wind.advance(1. / 12., next != previous, false, None);
        previous = next;
        app.elapsed += 1. / 12.;
        app.pulse = (app.pulse - 1. / 12. / 0.7).max(0.);
        capture(&app, &dir.join(format!("motion/frame-{frame:04}.png")), 1120, 753)?;
    }
    println!("Iced showcase captured in {}", dir.display());
    Ok(())
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
        b.command("family/2");
        assert!(!b.app.picked_distro);
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
    fn selection_is_explicit_for_each_family() {
        for i in 0..4 {
            let mut a = app();
            a.bridge.app.go(Step::System);
            a.bridge.next();
            assert_eq!(a.bridge.app.step, Step::System);
            a.bridge.command(&format!("family/{i}"));
            let (heroes, _) = a.bridge.tiles();
            assert!(heroes.iter().all(|t| !t.selected));
            a.bridge.next();
            assert_eq!(a.bridge.app.step, Step::Distribution);
        }
    }
    #[test]
    fn real_iced_family_button_dispatches_a_choice() {
        use iced_runtime::user_interface::{Cache, UserInterface};
        let mut a = app();
        a.bridge.app.go(Step::System);
        let mut renderer = iced_tiny_skia::Renderer::new(FACE, iced::Pixels(14.));
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
