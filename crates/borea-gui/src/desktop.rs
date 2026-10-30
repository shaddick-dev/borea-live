//! Slint presentation over the existing installer state machine and service clients.
use super::*;
use crate::install::Page;
use slint::{ComponentHandle, Model, ModelRc, SharedString, Timer, TimerMode, VecModel};
use std::{
    cell::RefCell,
    rc::Rc,
    time::{Duration, Instant},
};
slint::include_modules!();

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
fn strings(items: impl IntoIterator<Item = String>) -> ModelRc<SharedString> {
    Rc::new(VecModel::from(items.into_iter().map(Into::into).collect::<Vec<_>>())).into()
}
struct Bridge {
    app: LiveInstaller,
    ctx: egui::Context,
    network: Option<usize>,
    heroes: Vec<Tile>,
    groups: Vec<Section>,
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
        if self.busy() || self.app.firmware_dialog {
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
                        self.app.apply(&self.ctx, Action::HandoffChoose(pick));
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
            self.app.apply(&self.ctx, action);
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
                Page::Running if n.error.is_some() => n.retry_progress(),
                Page::Finished => n.reboot(),
                _ => {}
            }
        } else if self.app.step == Step::Review && self.app.handoff_target().is_some() {
            match self.app.handoff.phase {
                Phase::Choosing => self.app.handoff.review(),
                Phase::Reviewed(_) => self.app.apply(&self.ctx, Action::Next),
                Phase::Failed(_) => self.app.handoff.retry(),
                _ => {}
            }
        } else {
            self.app.apply(&self.ctx, Action::Next);
        }
    }
    fn back(&mut self) {
        self.pending_family = None;
        if self.busy() || self.app.firmware_dialog {
            return;
        }
        if self.native() && self.app.native.page != Page::Storage {
            let n = &mut self.app.native;
            let prior = n.page;
            n.invalidate();
            n.page = if prior == Page::Review { Page::Account } else { Page::Storage };
        } else {
            self.app.apply(&self.ctx, Action::Back);
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
                self.app.apply(&self.ctx, Action::HandoffChoose(pick));
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
    fn refresh(&mut self, ui: &BoreaWindow) {
        if self.app.step != Step::System {
            self.pending_family = None;
        }
        if let Some((family, at)) = self.pending_family
            && (self.app.still || at.elapsed() >= Duration::from_millis(280))
        {
            self.pending_family = None;
            self.app.apply(&self.ctx, Action::Family(family));
        }
        self.app.poll();
        let (heroes, groups) = self.tiles();
        if heroes != self.heroes {
            ui.set_heroes(update_tiles(ui, &ui.get_heroes(), &self.heroes, &heroes));
            self.heroes = heroes;
        }
        if groups != self.groups {
            let current = ui.get_groups();
            if groups.len() == self.groups.len()
                && groups.iter().zip(&self.groups).all(|(a, b)| a.title == b.title)
            {
                for (i, (group, old)) in groups.iter().zip(&self.groups).enumerate() {
                    if let Some(mut row) = current.row_data(i) {
                        let updated = update_tiles(ui, &row.choices, &old.choices, &group.choices);
                        if old.choices.len() != group.choices.len()
                            || old.choices.iter().zip(&group.choices).any(|(a, b)| a.key != b.key)
                        {
                            row.choices = updated;
                            if let Some(model) =
                                current.as_any().downcast_ref::<VecModel<ChoiceGroup>>()
                            {
                                model.set_row_data(i, row);
                            }
                        }
                    }
                }
            } else {
                ui.set_groups(
                    Rc::new(VecModel::from(
                        groups
                            .iter()
                            .map(|g| ChoiceGroup {
                                title: g.title.clone().into(),
                                choices: tiles_model(ui, &g.choices),
                            })
                            .collect::<Vec<_>>(),
                    ))
                    .into(),
                );
            }
            self.groups = groups;
        }
        let a = &self.app;
        ui.set_page((a.step.number() - 1) as i32);
        ui.set_reached((a.reached.number() - 1) as i32);
        ui.set_heading(a.step.title().into());
        ui.set_subtitle(a.step.lead().into());
        ui.set_summary(a.selection_title().into());
        ui.set_connection(a.wifi.connection_label().into());
        ui.set_dark(a.palette.dark);
        ui.global::<Design>().set_dark(a.palette.dark);
        ui.set_motion_paused(a.still);
        ui.global::<Design>().set_motion(!a.still && ui.get_motion_active());
        ui.set_locked(self.busy() || a.firmware_dialog);
        ui.set_firmware_dialog(a.firmware_dialog);
        ui.set_firmware_available(a.firmware.control(a.uefi()).0);
        ui.set_secure_boot_visible(a.step == Step::Review);
        ui.set_secure_boot_available(matches!(a.secure_boot(), Some(Readiness::Available { .. })));
        ui.set_secure_boot_note(
            match a.secure_boot() {
                Some(Readiness::Unavailable(reason)) => {
                    format!("Optional. Leave Secure Boot off: {reason}.")
                }
                Some(Readiness::Available { .. }) if a.selection.secure_boot => {
                    secure_boot::STEPS.join("\n")
                }
                _ => "Optional. Leave it off for machines without Secure Boot support.".into(),
            }
            .into(),
        );
        ui.set_secure_boot(a.selection.secure_boot);
        ui.set_notice("".into());
        ui.set_details("".into());
        ui.set_account(self.native() && a.native.page == Page::Account);
        ui.set_space_visible(false);
        ui.set_space_min(if self.native() { 40. } else { 32. });
        ui.set_confirmation_visible(false);
        ui.set_progress_visible(false);
        ui.set_next_label(if a.step == Step::Desktop { "Review" } else { "Continue" }.into());
        ui.set_next_enabled(a.can_continue() && !self.busy());
        if a.step == Step::Machine
            && let Some(hw) = a.hardware()
        {
            ui.set_details(
                borea_distro::machine_findings(hw)
                    .into_iter()
                    .map(|finding| match finding {
                        borea_distro::Finding::Recommend(text)
                        | borea_distro::Finding::Unknown(text)
                        | borea_distro::Finding::Blocker(text) => text,
                    })
                    .collect::<Vec<_>>()
                    .join("\n")
                    .into(),
            );
        }
        if a.step == Step::Connect {
            ui.set_notice(
                a.wifi.message.clone().or(a.wifi.unavailable.clone()).unwrap_or_default().into(),
            );
            ui.set_wifi_selected(self.network.is_some());
            ui.set_wifi_secured(self.network.and_then(|i| a.wifi.networks.get(i)).is_some_and(
                |n| {
                    !matches!(
                        n.security,
                        borea_network::Security::Open | borea_network::Security::EnhancedOpen
                    )
                },
            ));
        }
        if self.native() {
            let n = &a.native;
            ui.set_notice(n.error.clone().unwrap_or_default().into());
            ui.set_next_enabled(!n.loading());
            match n.page {
                Page::Storage => {
                    ui.set_heading("Where should Arch go?".into());
                    ui.set_next_label("Account and region".into());
                    ui.set_next_enabled(n.pick.is_some() && !n.loading());
                }
                Page::Account => {
                    ui.set_heading("Make it yours.".into());
                    ui.set_subtitle(
                        "Your account, region and applications. Every choice belongs to you."
                            .into(),
                    );
                    ui.set_account(true);
                    ui.set_next_label("Check installation plan".into());
                }
                Page::Review => {
                    ui.set_heading("Check your installation.".into());
                    ui.set_details(n.review_details().unwrap_or_default().into());
                    ui.set_next_label("Install Arch".into());
                    ui.set_next_enabled(n.ready());
                    ui.set_confirmation_visible(true);
                    ui.set_confirmed(n.confirmed);
                    ui.set_confirmation_label("Apply these disk changes and install Arch. I have backed up anything important.".into());
                }
                Page::Running => {
                    ui.set_heading("Installing Arch.".into());
                    ui.set_details(n.stage.clone().into());
                    ui.set_progress_visible(true);
                    ui.set_progress(n.progress);
                    ui.set_next_label("Retry / reconnect".into());
                    ui.set_next_enabled(n.error.is_some());
                }
                Page::Finished => {
                    ui.set_heading("Ready for a fresh start.".into());
                    ui.set_details("Restart, unlock the disk and sign in through Noctalia Greeter. Extra apps finish quietly after login.".into());
                    ui.set_next_label("Restart into Arch".into());
                    ui.set_next_enabled(true);
                }
            }
            update_space(ui, &n.disks, &n.pick);
            if n.page != Page::Account {
                ui.set_password("".into());
                ui.set_password_confirmation("".into());
            }
        } else if a.step == Step::Review && a.handoff_target().is_some() {
            let h = &a.handoff;
            match &h.phase {
                Phase::Idle | Phase::Loading => {
                    ui.set_next_enabled(false);
                    ui.set_notice("Looking for installation drives…".into());
                }
                Phase::Choosing => {
                    ui.set_heading("Where should Omarchy go?".into());
                    ui.set_next_label("Check this plan".into());
                    ui.set_next_enabled(h.pick.is_some());
                }
                Phase::Reviewing => {
                    ui.set_next_label("Checking…".into());
                    ui.set_next_enabled(false);
                }
                Phase::Reviewed(review) => {
                    ui.set_details(
                        format!("{}\n\n{}", review.steps.join("\n"), review.choose).into(),
                    );
                    ui.set_confirmation_visible(true);
                    ui.set_confirmation_label(
                        "Apply these disk changes. I have backed up anything important.".into(),
                    );
                    ui.set_confirmed(h.confirmed);
                    ui.set_next_label("Begin installation".into());
                    ui.set_next_enabled(h.ready());
                }
                Phase::Running { stage, fraction, .. } => {
                    ui.set_heading("Getting Omarchy ready.".into());
                    ui.set_details(stage.clone().into());
                    ui.set_progress_visible(true);
                    ui.set_progress(fraction.unwrap_or(0.));
                    ui.set_next_enabled(false);
                }
                Phase::Finished { choose } => {
                    ui.set_heading("Restarting into Omarchy.".into());
                    ui.set_details(choose.clone().into());
                    ui.set_next_enabled(false);
                }
                Phase::Failed(e) => {
                    ui.set_notice(e.clone().into());
                    ui.set_next_label("Try again".into());
                    ui.set_next_enabled(true);
                }
            }
            update_space(ui, &h.disks, &h.pick);
        } else if a.step == Step::Review {
            ui.set_notice(
                "This target is a preview. Its installation pathway is not connected yet.".into(),
            );
            ui.set_details(a.selection.title(&a.catalog, a.family()).into());
            ui.set_next_label("Not available yet".into());
            ui.set_next_enabled(false);
        }
        if a.firmware_dialog {
            ui.set_next_enabled(false);
            if let Some(error) = &a.firmware.error {
                ui.set_notice(error.clone().into());
            }
        }
        if self.pending_family.is_some() {
            ui.set_next_enabled(false);
        }
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
fn tiles_model(ui: &BoreaWindow, tiles: &[Tile]) -> ModelRc<ChoiceData> {
    Rc::new(VecModel::from(
        tiles
            .iter()
            .map(|t| ChoiceData {
                key: t.key.clone().into(),
                label: t.label.clone().into(),
                description: t.description.clone().into(),
                meta: t.meta.clone().into(),
                logo: logo(ui, &t.logo),
                selected: t.selected,
                enabled: t.enabled,
            })
            .collect::<Vec<_>>(),
    ))
    .into()
}

/// Keep item identities (and their focus and animation state) across a pick.
fn update_tiles(
    ui: &BoreaWindow,
    current: &ModelRc<ChoiceData>,
    old: &[Tile],
    next: &[Tile],
) -> ModelRc<ChoiceData> {
    if old.len() == next.len()
        && old.iter().zip(next).all(|(a, b)| a.key == b.key)
        && let Some(model) = current.as_any().downcast_ref::<VecModel<ChoiceData>>()
    {
        for (i, (a, b)) in old.iter().zip(next).enumerate() {
            if a != b {
                model.set_row_data(i, tile_data(ui, b));
            }
        }
        return current.clone();
    }
    tiles_model(ui, next)
}

fn tile_data(ui: &BoreaWindow, t: &Tile) -> ChoiceData {
    ChoiceData {
        key: t.key.clone().into(),
        label: t.label.clone().into(),
        description: t.description.clone().into(),
        meta: t.meta.clone().into(),
        logo: logo(ui, &t.logo),
        selected: t.selected,
        enabled: t.enabled,
    }
}

fn wind_frame(
    ui: &BoreaWindow,
    motion: &mut crate::frost_motion::FrostMotion,
    dt: f64,
    changed: bool,
    paused: bool,
    progress: Option<f32>,
) {
    motion.advance(dt, changed, paused, progress);
    ui.set_motion_active(!paused);
    ui.global::<Design>().set_motion(!paused);
    if !paused || ui.get_wind().row_count() == 0 {
        ui.set_wind(
            Rc::new(VecModel::from(
                motion
                    .trails()
                    .into_iter()
                    .map(|g| WindData {
                        x: g.x,
                        y: g.y,
                        length: g.length,
                        height: g.height,
                        alpha: g.alpha,
                    })
                    .collect::<Vec<_>>(),
            ))
            .into(),
        );
    }
}

fn view_key(b: &Bridge) -> String {
    format!(
        "{:?}:{:?}:{:?}:{:?}",
        b.app.step,
        b.app.native.page,
        std::mem::discriminant(&b.app.handoff.phase),
        b.pending_family.map(|(f, _)| f)
    )
}
fn logo(ui: &BoreaWindow, id: &str) -> slint::Image {
    let logos = ui.global::<Logos>();
    match id {
        "arch" => logos.get_arch(),
        "fedora" => logos.get_fedora(),
        "debian" => logos.get_debian(),
        "opensuse" | "tumbleweed" => logos.get_opensuse(),
        "cachyos" => logos.get_cachyos(),
        "omarchy" => logos.get_omarchy(),
        "bazzite" => logos.get_bazzite(),
        "nobara" => logos.get_nobara(),
        "pikaos" => logos.get_pikaos(),
        _ => slint::Image::default(),
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
fn update_space(ui: &BoreaWindow, disks: &[handoff::Disk], pick: &Option<Where>) {
    if let Some(Where::Alongside { disk, part, size }) = pick
        && let Some(max) =
            disks.get(*disk).and_then(|d| d.parts.get(*part)).and_then(|p| p.max_linux_size)
    {
        ui.set_space_visible(true);
        ui.set_space_max((max / handoff::GIB) as f32);
        ui.set_space((*size / handoff::GIB) as f32);
    }
}
fn connect(ui: &BoreaWindow, state: Rc<RefCell<Bridge>>) {
    macro_rules! callback{($name:ident,|$b:ident $(,$arg:ident)*|$body:block)=>{{let weak=ui.as_weak();let state=state.clone();ui.$name(move|$($arg),*|{if let Some(ui)=weak.upgrade(){let mut $b=state.borrow_mut();$body $b.refresh(&ui);}});}};}
    callback!(on_choose, |b, key| {
        b.command(key.as_str());
    });
    callback!(on_advance, |b| {
        b.next();
    });
    callback!(on_back, |b| {
        b.back();
    });
    callback!(on_navigate, |b, index| {
        b.pending_family = None;
        if let Ok(i) = usize::try_from(index)
            && let Some(step) = Step::ALL.get(i)
        {
            let ctx = b.ctx.clone();
            b.app.apply(&ctx, Action::Go(*step));
        }
    });
    callback!(on_theme, |b| {
        let ctx = b.ctx.clone();
        b.app.apply(&ctx, Action::ToggleTheme);
    });
    callback!(on_motion, |b| {
        let ctx = b.ctx.clone();
        b.app.apply(&ctx, Action::ToggleMotion);
    });
    callback!(on_change_field, |b, field, value| {
        b.app.native.edit_account(field.as_str(), value.to_string());
    });
    callback!(on_region, |b, kind, index| {
        b.region(kind.as_str(), index);
    });
    callback!(on_set_space, |b, value| {
        b.space(value);
    });
    callback!(on_confirm, |b, on| {
        if b.native() {
            b.app.native.confirmed = on;
        } else {
            b.app.handoff.confirm(on);
        }
    });
    callback!(on_set_secure_boot, |b, on| {
        if !on || matches!(b.app.secure_boot(), Some(Readiness::Available { .. })) {
            let ctx = b.ctx.clone();
            b.app.apply(&ctx, Action::SecureBoot(on));
        }
    });
    callback!(on_firmware, |b, action| {
        let ctx = b.ctx.clone();
        b.app.apply(
            &ctx,
            match action.as_str() {
                "open" => Action::OpenFirmware,
                "restart" => Action::RestartFirmware,
                _ => Action::CancelFirmware,
            },
        );
    });
    let weak = ui.as_weak();
    let wifi_state = state.clone();
    ui.on_wifi(move |action| {
        if let Some(ui) = weak.upgrade() {
            let mut b = wifi_state.borrow_mut();
            if !b.busy() {
                match action.as_str() {
                    "rescan" => b.app.wifi.send(crate::wifi::Command::Rescan),
                    "join" => {
                        let password = zeroize::Zeroizing::new(ui.get_wifi_password().to_string());
                        let secret = if password.is_empty() {
                            None
                        } else {
                            Some(borea_network::Secret::new(password.to_string()))
                        };
                        if ui.get_hidden_network() {
                            b.app.wifi.send(crate::wifi::Command::JoinHidden {
                                ssid: ui.get_hidden_ssid().to_string(),
                                security: match ui.get_hidden_security() {
                                    1 => borea_network::Security::Wpa3Personal,
                                    2 => borea_network::Security::Open,
                                    _ => borea_network::Security::WpaPersonal,
                                },
                                passphrase: secret,
                            });
                        } else if let Some(n) =
                            b.network.and_then(|i| b.app.wifi.networks.get(i)).cloned()
                        {
                            b.app.wifi.send(crate::wifi::Command::Join {
                                network: n,
                                passphrase: secret,
                            });
                        }
                        ui.set_wifi_password("".into());
                    }
                    _ => {}
                }
            }
            b.refresh(&ui);
        }
    });
    ui.set_languages(strings(
        borea_locale::LOCALES.iter().map(|v| format!("{} · {}", v.english, v.id)),
    ));
    ui.set_keyboards(strings(borea_locale::KEYBOARDS.iter().map(|v| v.description.to_owned())));
    ui.set_consoles(strings(borea_locale::console_keymaps().map(|v| format!("Disk unlock · {v}"))));
    ui.set_timezones(strings(borea_locale::TIMEZONES.iter().map(|v| format!("Time zone · {v}"))));
}

fn start_motion(ui: &BoreaWindow, state: Rc<RefCell<Bridge>>) -> Timer {
    let motion_timer = Timer::default();
    let motion_ui = ui.as_weak();
    let motion_state = state.clone();
    let mut wind = crate::frost_motion::FrostMotion::default();
    let mut last_frame = Instant::now();
    let mut last_view = view_key(&state.borrow());
    motion_timer.start(TimerMode::Repeated, crate::wind::FRAME, move || {
        if let Some(ui) = motion_ui.upgrade() {
            let now = Instant::now();
            let dt = now.duration_since(last_frame).as_secs_f64();
            last_frame = now;
            let b = motion_state.borrow();
            let view = view_key(&b);
            let paused = b.app.still || !ui.window().is_visible() || ui.window().is_minimized();
            let progress = (b.app.native.page == Page::Running).then_some(b.app.native.progress);
            wind_frame(&ui, &mut wind, dt, view != last_view, paused, progress);
            last_view = view;
        }
    });
    motion_timer
}

pub fn run(live: bool) -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().collect();
    if let Some(i) = args.iter().position(|a| a == "--showcase") {
        return showcase(std::path::Path::new(
            args.get(i + 1).ok_or("--showcase needs an output directory")?,
        ));
    }
    let ctx = egui::Context::default();
    let app = if args.iter().any(|a| a == "--preview") {
        LiveInstaller::with_hardware(&ctx, borea_hardware::scan(), true)
    } else {
        LiveInstaller::for_desktop(ctx.clone())
    };
    let ui = BoreaWindow::new()?;
    ui.set_product(if live { "Borea Live" } else { "Windpad" }.into());
    let state = Rc::new(RefCell::new(Bridge {
        app,
        ctx,
        network: None,
        heroes: vec![],
        groups: vec![],
        pending_family: None,
    }));
    connect(&ui, state.clone());
    state.borrow_mut().refresh(&ui);
    let _motion_timer = start_motion(&ui, state.clone());
    let weak = ui.as_weak();
    let smoke = args.iter().any(|a| a == "--boot-smoke");
    let ticks = Rc::new(RefCell::new(0u32));
    let timer = Timer::default();
    timer.start(TimerMode::Repeated, Duration::from_millis(150), move || {
        if let Some(ui) = weak.upgrade() {
            state.borrow_mut().refresh(&ui);
            *ticks.borrow_mut() += 1;
            if smoke && *ticks.borrow() >= 3 {
                println!("BOREA_GUI_READY");
                let _ = ui.hide();
                let _ = slint::quit_event_loop();
            }
        }
    });
    if live && !args.iter().any(|a| a == "--window") {
        ui.window().set_fullscreen(true);
    }
    ui.run()?;
    Ok(())
}

#[path = "../tests/common/mod.rs"]
mod showcase_fixture;

/// Render actual Slint components without a display server or installer service.
fn showcase(dir: &std::path::Path) -> Result<(), Box<dyn std::error::Error>> {
    use slint::platform::{
        Platform, WindowAdapter,
        software_renderer::{MinimalSoftwareWindow, RepaintBufferType},
    };
    struct Headless(Rc<MinimalSoftwareWindow>, Rc<std::cell::Cell<Duration>>);
    impl Platform for Headless {
        fn create_window_adapter(&self) -> Result<Rc<dyn WindowAdapter>, slint::PlatformError> {
            Ok(self.0.clone())
        }
        fn duration_since_start(&self) -> Duration {
            self.1.get()
        }
    }
    let window = MinimalSoftwareWindow::new(RepaintBufferType::NewBuffer);
    let clock = Rc::new(std::cell::Cell::new(Duration::ZERO));
    slint::platform::set_platform(Box::new(Headless(window.clone(), clock.clone())))?;
    let ui = BoreaWindow::new()?;
    let ctx = egui::Context::default();
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
    let state = Rc::new(RefCell::new(Bridge {
        app: LiveInstaller::with_link(&ctx, showcase_fixture::machine(), wifi, true),
        ctx,
        network: Some(0),
        heroes: vec![],
        groups: vec![],
        pending_family: None,
    }));
    connect(&ui, state.clone());
    window.set_size(slint::PhysicalSize::new(1280, 860));
    ui.show()?;
    std::fs::create_dir_all(dir)?;
    let mut still_wind = crate::frost_motion::FrostMotion::default();
    wind_frame(&ui, &mut still_wind, 0., false, true, None);
    let capture = |name: &str| -> Result<(), Box<dyn std::error::Error>> {
        state.borrow_mut().refresh(&ui);
        clock.set(clock.get() + Duration::from_millis(83));
        slint::platform::update_timers_and_animations();
        let size = window.size();
        let mut pixels = vec![slint::Rgb8Pixel::default(); (size.width * size.height) as usize];
        window.request_redraw();
        window.draw_if_needed(|renderer| {
            renderer.render(&mut pixels, size.width as usize);
        });
        let data: Vec<u8> = pixels.iter().flat_map(|p| [p.r, p.g, p.b]).collect();
        let file = std::fs::File::create(dir.join(format!("{name}.png")))?;
        let mut png = png::Encoder::new(file, size.width, size.height);
        png.set_color(png::ColorType::Rgb);
        png.set_depth(png::BitDepth::Eight);
        png.write_header()?.write_image_data(&data)?;
        Ok(())
    };
    for dark in [true, false] {
        {
            let mut b = state.borrow_mut();
            b.app.palette = if dark { DARK } else { LIGHT };
            b.app = LiveInstaller::with_link(
                &b.ctx,
                showcase_fixture::machine(),
                WifiLink::simulated(
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
                ),
                dark,
            );
        }
        let mode = if dark { "dark" } else { "light" };
        capture(&format!("{mode}-1-connect"))?;
        {
            let mut b = state.borrow_mut();
            b.app.go(Step::Machine);
        }
        capture(&format!("{mode}-2-machine"))?;
        {
            let mut b = state.borrow_mut();
            b.app.go(Step::System);
        }
        capture(&format!("{mode}-3-family"))?;
        ui.invoke_choose("family/0".into());
        let before = ui.get_heroes();
        ui.invoke_choose("distro/1".into());
        assert!(
            std::ptr::addr_eq(before.as_any(), ui.get_heroes().as_any()),
            "selection must retain card instances"
        );
        capture(&format!("{mode}-4-distribution"))?;
        ui.invoke_choose("edition/2".into());
        ui.invoke_advance();
        ui.invoke_choose("mode/compose".into());
        ui.invoke_choose("compositor/hyprland".into());
        ui.invoke_choose("shell/noctalia".into());
        capture(&format!("{mode}-5-desktop"))?;
        ui.invoke_choose("extra/gaming".into());
        ui.set_scroll_bottom(true);
        capture(&format!("{mode}-5-extras"))?;
        ui.invoke_advance();
        ui.set_scroll_bottom(false);
        capture(&format!("{mode}-6-review"))?;
    }
    ui.invoke_navigate(2);
    ui.set_product("Windpad".into());
    capture("windpad-family")?;
    window.set_size(slint::PhysicalSize::new(900, 600));
    capture("small-family")?;
    window.set_size(slint::PhysicalSize::new(1280, 860));
    ui.set_product("Borea Live".into());
    state.borrow_mut().app.set_install_service(std::sync::Arc::new(|method,_| {
        if method == "disks.list" { Ok(serde_json::json!({"disks":[{
            "id":{"logical_sector":512,"path":"/dev/vda","serial":"SHOWCASE","size":512000000000u64,"model":"Example NVMe"},
            "readonly":false,"in_use":false,"live":false,"parts":[{"path":"/dev/vda3","number":3,"uuid":"EXAMPLE-WINDOWS","fs":"ntfs","max_linux_size":200*handoff::GIB}]
        }]})) } else { Err("Showcase mode cannot approve or start an installation".into()) }
    }));
    ui.invoke_choose("family/0".into());
    ui.invoke_choose("distro/0".into());
    ui.invoke_choose("edition/3".into());
    ui.invoke_advance();
    ui.invoke_choose("mode/compose".into());
    ui.invoke_choose("compositor/hyprland".into());
    ui.invoke_choose("shell/noctalia".into());
    ui.invoke_advance();
    for _ in 0..20 {
        state.borrow_mut().app.poll();
        std::thread::sleep(Duration::from_millis(2));
    }
    capture("native-storage")?;
    assert!(state.borrow().app.native.pick.is_none());
    ui.invoke_choose("whole/0".into());
    ui.invoke_advance();
    capture("native-account")?;
    if std::env::args().any(|a| a == "--animate") {
        std::fs::create_dir_all(dir.join("motion"))?;
        {
            let mut b = state.borrow_mut();
            b.app = LiveInstaller::with_hardware(&b.ctx, showcase_fixture::machine(), true);
            b.app.still = false;
            b.pending_family = None;
        }
        ui.set_scroll_bottom(false);
        ui.set_motion_active(true);
        state.borrow_mut().refresh(&ui);
        let mut wind = crate::frost_motion::FrostMotion::default();
        let mut view = view_key(&state.borrow());
        // Actual Slint frames at 12 fps, driven by the same motion engine with
        // a virtual monotonic clock. No sleeps, synthetic fades or disk service.
        for frame in 0..180 {
            match frame {
                12 => state.borrow_mut().app.go(Step::Machine),
                24 => state.borrow_mut().app.go(Step::System),
                45 => {
                    state.borrow_mut().pending_family = Some((Family::Arch, Instant::now()));
                }
                49 => {
                    let mut b = state.borrow_mut();
                    b.pending_family = None;
                    let ctx = b.ctx.clone();
                    b.app.apply(&ctx, Action::Family(Family::Arch));
                }
                56 => ui.invoke_choose("distro/1".into()),
                84 => {
                    ui.invoke_choose("edition/2".into());
                    ui.invoke_advance();
                }
                92 => ui.invoke_choose("mode/compose".into()),
                100 => ui.invoke_choose("compositor/hyprland".into()),
                112 => ui.invoke_choose("shell/noctalia".into()),
                124 => {
                    ui.invoke_choose("extra/gaming".into());
                    ui.set_scroll_bottom(true);
                }
                152 => {
                    ui.invoke_advance();
                    ui.set_scroll_bottom(false);
                }
                _ => {}
            }
            let next = view_key(&state.borrow());
            wind_frame(&ui, &mut wind, 1. / 12., next != view, false, None);
            view = next;
            capture(&format!("motion/frame-{frame:04}"))?;
        }
    }
    ui.hide()?;
    println!("Slint showcase captured in {}", dir.display());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn bridge() -> Bridge {
        let ctx = egui::Context::default();
        Bridge {
            app: LiveInstaller::with_hardware(&ctx, showcase_fixture::machine(), true),
            ctx,
            network: None,
            heroes: vec![],
            groups: vec![],
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
    fn slint_commands_require_every_choice_and_reset_later_choices() {
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
    fn slint_account_and_region_reach_the_reviewed_plan() {
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
