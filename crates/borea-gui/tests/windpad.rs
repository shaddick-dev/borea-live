//! Drive the real Windpad window through its accessibility tree, as a person would.

mod common;

use common::machine;

use borea_core::Family;
use borea_gui::Windpad;
use borea_gui::app::{Dialog, Step};
use eframe::egui::{self, accesskit::Role};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;

type Windowed = Harness<'static, Option<Windpad>>;

fn harness(dark: bool) -> Windowed {
    sized(dark, 1200.0)
}

fn sized(dark: bool, width: f32) -> Windowed {
    let mut harness = Harness::builder().with_size(egui::vec2(width, 1500.0)).build_ui_state(
        move |ui, app: &mut Option<Windpad>| {
            let ctx = ui.ctx().clone();
            let app = app.get_or_insert_with(|| Windpad::with_hardware(&ctx, machine(), dark));
            for action in app.frame(ui) {
                app.apply(&ctx, action);
            }
        },
        None,
    );
    harness.run();
    harness
}

/// Bring a button into view, click it by its accessible name, and let the window settle.
fn press(h: &mut Windowed, label: &str) {
    h.get_by_role_and_label(Role::Button, label).scroll_to_me();
    h.run();
    h.get_by_role_and_label(Role::Button, label).click();
    h.run();
}

fn app(h: &Windowed) -> &Windpad {
    h.state().as_ref().unwrap()
}

fn title(h: &Windowed, family: Family) -> String {
    app(h).selection().title(app(h).catalog(), family)
}

const CONTINUE: &str = "Continue with this selection";

/// Cards start on a ready-made desktop; this takes Arch to Minimal, to compose.
fn arch_minimal(h: &mut Windowed) {
    press(h, "Or compose your own desktop on Arch Linux Minimal");
    assert!(app(h).selection().composable(app(h).catalog(), Family::Arch));
}

#[test]
fn minimal_path_previews_only_after_use_this_desktop() {
    let mut h = harness(false);
    assert_eq!(title(&h, Family::Arch), "Arch Linux · KDE Plasma");
    assert!(h.query_by_label(CONTINUE).is_some(), "a ready-made desktop previews at once");
    arch_minimal(&mut h);
    assert!(h.query_by_label(CONTINUE).is_none(), "composing previews once confirmed");

    press(&mut h, "Choose compositor for Arch Linux, currently Hyprland");
    assert_eq!(app(&h).dialog(), Some(Dialog::Compose(Family::Arch, Step::Compositor)));
    press(&mut h, "niri");
    assert_eq!(
        app(&h).dialog(),
        Some(Dialog::Compose(Family::Arch, Step::Shell)),
        "picking advances"
    );
    press(&mut h, "Dux Shell");
    assert!(h.query_by_label(CONTINUE).is_none(), "still unconfirmed");
    press(&mut h, "Use this desktop");

    assert_eq!(app(&h).dialog(), None);
    assert_eq!(title(&h, Family::Arch), "Arch Linux · Minimal · niri + Dux Shell");
    press(&mut h, CONTINUE);
    assert_eq!(app(&h).dialog(), Some(Dialog::Continue(Family::Arch)));
    press(&mut h, "Keep exploring");
    assert_eq!(app(&h).dialog(), None);
}

#[test]
fn no_desktop_and_no_shell_are_complete_paths() {
    let mut h = harness(false);
    arch_minimal(&mut h);
    h.get_by_role_and_label(Role::RadioButton, "No desktop").click();
    h.run();
    assert!(h.query_by_label(CONTINUE).is_some());
    assert_eq!(title(&h, Family::Arch), "Arch Linux · Minimal · no desktop");

    h.get_by_role_and_label(Role::RadioButton, "Compose a desktop").click();
    h.run();
    press(&mut h, "Choose shell for Arch Linux, currently Noctalia");
    press(&mut h, "No shell");
    press(&mut h, "Use this desktop");
    assert_eq!(title(&h, Family::Arch), "Arch Linux · Minimal · Hyprland");

    press(&mut h, "Choose compositor for Arch Linux, currently Hyprland");
    press(&mut h, "Skip · no desktop");
    assert_eq!(app(&h).dialog(), None);
    assert_eq!(title(&h, Family::Arch), "Arch Linux · Minimal · no desktop");
}

#[test]
fn every_card_has_extras_in_your_desktop() {
    let mut h = harness(false);
    let gaming = |h: &Windowed, f| app(h).selection().extra(app(h).catalog(), f, "gaming");
    assert!(!gaming(&h, Family::Arch), "unticked until chosen");
    assert!(gaming(&h, Family::Opensuse), "Tumbleweed ticks it by default");
    for distro in ["Arch Linux", "Fedora Linux", "Debian", "openSUSE Tumbleweed"] {
        for extra in ["Gaming", "Media codecs", "Flatpak", "Printing"] {
            let label = format!("{extra} for {distro}");
            assert!(h.query_by_label(&label).is_some(), "{label}");
        }
    }

    arch_minimal(&mut h);
    let check = "Gaming for Arch Linux";
    h.get_by_role_and_label(Role::CheckBox, check).scroll_to_me();
    h.run();
    h.get_by_role_and_label(Role::CheckBox, check).click();
    h.run();
    assert!(gaming(&h, Family::Arch));
    h.get_by_role_and_label(Role::RadioButton, "No desktop").click();
    h.run();
    assert!(h.query_by_label_contains("a console login plus Gaming.").is_some());
    let rows = app(&h).selection().rows(app(&h).catalog(), Family::Arch);
    assert!(rows.contains(&("Extras", "Gaming".into())));
}

/// Most people pick a desktop edition first, so every such card points to composing.
#[test]
fn desktop_editions_offer_the_compose_path() {
    let mut h = harness(false);
    assert_eq!(title(&h, Family::Fedora), "Fedora Linux · GNOME · Workstation");
    press(&mut h, "Or compose your own desktop on Fedora Linux Minimal");
    assert_eq!(app(&h).selection().selected, Family::Fedora);
    assert!(app(&h).selection().composable(app(&h).catalog(), Family::Fedora));
    assert!(h.query_by_label("Or compose your own desktop on Fedora Linux Minimal").is_none());
    assert!(h.query_by_label("Choose shell for Fedora Linux, currently Noctalia").is_some());
}

/// Four cards in a row when they fit, else two, else one; a row's cards end level.
#[test]
fn cards_flow_with_the_width() {
    let top = |h: &Windowed, family: &str| {
        h.get_by_label(&format!("Select {family} family")).rect().top()
    };
    let level = |a: f32, b: f32| (a - b).abs() < 2.0;
    let wide = sized(false, 1900.0);
    assert!(level(top(&wide, "Arch"), top(&wide, "openSUSE")), "one row of four");
    let medium = sized(false, 1200.0);
    assert!(level(top(&medium, "Arch"), top(&medium, "Fedora")));
    assert!(top(&medium, "Debian") > top(&medium, "Arch") + 100.0, "two rows of two");
    let narrow = sized(false, 700.0);
    assert!(top(&narrow, "Fedora") > top(&narrow, "Arch"), "one column");
}

#[test]
fn desktop_editions_preview_when_their_card_is_chosen() {
    let mut h = harness(false);
    press(&mut h, "Select Fedora family");
    assert_eq!(app(&h).selection().selected, Family::Fedora);
    assert!(h.query_by_label(CONTINUE).is_some());
}

#[test]
fn dialogs_open_and_close() {
    let mut h = harness(true);
    press(&mut h, "Hardware");
    assert_eq!(app(&h).dialog(), Some(Dialog::Hardware));
    assert!(h.query_by_label("None selected").is_some(), "no disk is ever preselected");
    press(&mut h, "Close");
    press(&mut h, "Wi-Fi");
    assert_eq!(app(&h).dialog(), Some(Dialog::Wifi));
    assert!(h.query_by_label_contains("Wi-Fi is unavailable here").is_some());
    press(&mut h, "Close");
    assert_eq!(app(&h).dialog(), None);
}

/// A shell scrolled out of view must never catch a click meant for the dialog footer.
#[test]
fn hidden_cards_never_steal_footer_clicks() {
    let mut h = harness(false);
    arch_minimal(&mut h);
    press(&mut h, "Choose shell for Arch Linux, currently Noctalia");
    // Without scrolling, click straight through to the footer.
    h.get_by_role_and_label(Role::Button, "Use this desktop").click();
    h.run();
    assert_eq!(app(&h).dialog(), None);
    assert!(h.query_by_label(CONTINUE).is_some());
}

/// Set WINDPAD_SHOTS to a directory to save screenshots for review.
#[test]
fn renders_light_and_dark() {
    let Ok(dir) = std::env::var("WINDPAD_SHOTS") else { return };
    for width in [1900.0, 700.0] {
        let mut h = sized(true, width);
        h.render().unwrap().save(format!("{dir}/windpad-width-{width}.png")).unwrap();
    }
    for (dark, name) in [(false, "light"), (true, "dark")] {
        let mut h = harness(dark);
        h.render().unwrap().save(format!("{dir}/windpad-{name}-start.png")).unwrap();
        arch_minimal(&mut h);
        press(&mut h, "Choose shell for Arch Linux, currently Noctalia");
        press(&mut h, "Use this desktop");
        h.render().unwrap().save(format!("{dir}/windpad-{name}.png")).unwrap();
        press(&mut h, "Choose compositor for Arch Linux, currently Hyprland");
        h.render().unwrap().save(format!("{dir}/windpad-{name}-compositor.png")).unwrap();
        press(&mut h, "MangoWC");
        h.render().unwrap().save(format!("{dir}/windpad-{name}-shell.png")).unwrap();
    }
}
