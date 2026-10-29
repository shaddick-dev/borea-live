//! Walk the Borea Live installer through its steps, as a person would.

mod common;

use borea_gui::LiveInstaller;
use borea_gui::live::{Action, Step};
use borea_live_core::Family;
use common::machine;
use eframe::egui::{self, accesskit::Role};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;

type Installer = Harness<'static, Option<LiveInstaller>>;

fn harness(dark: bool) -> Installer {
    with_hardware(dark, machine())
}

fn with_hardware(dark: bool, hardware: borea_hardware::HardwareSnapshot) -> Installer {
    let mut harness = Harness::builder().with_size(egui::vec2(1280.0, 860.0)).build_ui_state(
        move |ui, app: &mut Option<LiveInstaller>| {
            let ctx = ui.ctx().clone();
            let app = app
                .get_or_insert_with(|| LiveInstaller::with_hardware(&ctx, hardware.clone(), dark));
            for action in app.frame(ui) {
                app.apply(&ctx, action);
            }
        },
        None,
    );
    harness.run();
    harness
}

#[test]
fn firmware_footer_requires_confirmation_and_preview_never_reboots_host() {
    let mut h = harness(false);
    if let Ok(dir) = std::env::var("WINDPAD_SHOTS") {
        h.render().unwrap().save(format!("{dir}/firmware-footer.png")).unwrap();
    }
    press(&mut h, "UEFI setup");
    assert!(h.query_by_label("Restart into UEFI setup?").is_some());
    if let Ok(dir) = std::env::var("WINDPAD_SHOTS") {
        h.render().unwrap().save(format!("{dir}/firmware-confirmation.png")).unwrap();
    }
    press(&mut h, "Cancel");
    assert!(h.query_by_label("Restart into UEFI setup?").is_none());
    assert_eq!(app(&h).step(), Step::Connect);
    press(&mut h, "UEFI setup");
    press(&mut h, "Restart into firmware");
    assert!(h.query_by_label_contains("Firmware restart is unavailable in this preview").is_some());
    press(&mut h, "Cancel");
}

#[test]
fn legacy_bios_cannot_request_a_firmware_restart() {
    let mut hardware = machine();
    hardware.firmware = borea_live_core::Fact::Known(borea_live_core::FirmwareMode::Bios);
    let mut h = with_hardware(false, hardware);
    press(&mut h, "UEFI setup");
    assert!(h.query_by_label("Restart into UEFI setup?").is_none());
    assert_eq!(app(&h).step(), Step::Connect);
}

fn press(h: &mut Installer, label: &str) {
    // Some buttons appear only once a background answer (disks, a review) lands.
    for _ in 0..300 {
        if h.query_by_role_and_label(Role::Button, label).is_some() {
            break;
        }
        h.run();
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    h.get_by_role_and_label(Role::Button, label).scroll_to_me();
    h.run();
    h.get_by_role_and_label(Role::Button, label).click();
    h.run();
}

fn app(h: &Installer) -> &LiveInstaller {
    h.state().as_ref().unwrap()
}

fn title(h: &Installer) -> String {
    let app = app(h);
    app.selection().title(app.catalog(), app.selection().selected)
}

#[test]
fn guided_path_through_every_step() {
    let mut h = harness(true);
    assert_eq!(app(&h).step(), Step::Connect);
    assert!(h.query_by_label_contains("Wi-Fi is unavailable here").is_some());
    press(&mut h, "Next");
    assert_eq!(app(&h).step(), Step::Machine);
    assert!(h.query_by_label("Available to choose later").is_some());
    assert!(h.query_by_label("Installation medium").is_some(), "the live USB is never a target");
    press(&mut h, "Next");

    assert_eq!(app(&h).step(), Step::System);
    // The family cards carry Windpad's lines.
    assert!(h.query_by_label("A system shaped by you.").is_some());
    assert!(h.query_by_label("A fresh perspective.").is_some());
    press(&mut h, "Fedora family");
    assert_eq!(app(&h).step(), Step::Distribution);
    press(&mut h, "Fedora Linux");
    press(&mut h, "Minimal");
    assert!(h.query_by_label("Fedora Linux · Minimal · Choose a desktop").is_some());
    press(&mut h, "Next");

    assert_eq!(app(&h).step(), Step::Desktop);
    press(&mut h, "Compose a desktop");
    press(&mut h, "niri");
    press(&mut h, "No shell");
    press(&mut h, "Review");
    assert_eq!(app(&h).step(), Step::Review);
    assert_eq!(title(&h), "Fedora Linux · Minimal · niri");
    assert!(
        app(&h).selection().path_complete(app(&h).catalog(), Family::Fedora),
        "leaving Desktop confirms it"
    );

    // Prototype status shows before the button, and the button cannot be pressed.
    assert!(h.query_by_label_contains("This is a preview build: it cannot install yet").is_some());
    assert!(h.query_by_label("Not in this preview build").is_some());
    press(&mut h, "Begin installation");
    assert_eq!(app(&h).step(), Step::Review, "a disabled button does nothing");
}

#[test]
fn rail_returns_to_earlier_steps_and_change_resets_progress() {
    let mut h = harness(false);
    press(&mut h, "Next");
    press(&mut h, "Next");
    press(&mut h, "Arch family");
    press(&mut h, "Arch Linux");
    press(&mut h, "KDE Plasma");
    press(&mut h, "Next");
    press(&mut h, "Review");
    assert_eq!(app(&h).step(), Step::Review);

    press(&mut h, "Step 3: Family");
    assert_eq!(app(&h).step(), Step::System);
    press(&mut h, "Debian family");
    // Review is no longer reachable from the rail until Desktop is passed again.
    press(&mut h, "Step 6: Review");
    assert_eq!(app(&h).step(), Step::Distribution);
    assert!(!app(&h).can_continue());
}

#[test]
fn desktop_editions_skip_the_choice_and_bare_is_complete() {
    let mut h = harness(false);
    press(&mut h, "Next");
    press(&mut h, "Next");
    press(&mut h, "Fedora family");
    press(&mut h, "Fedora Linux");
    press(&mut h, "GNOME · Workstation");
    press(&mut h, "Next");
    assert!(h.query_by_label_contains("comes with its own desktop").is_some());

    press(&mut h, "Step 3: Family");
    press(&mut h, "Arch family");
    press(&mut h, "CachyOS");
    press(&mut h, "Minimal");
    press(&mut h, "Next");
    press(&mut h, "No desktop");
    let selection =
        |h: &Installer| app(h).selection().extra(app(h).catalog(), Family::Arch, "gaming");
    assert!(!selection(&h), "Gaming starts unticked");
    h.get_by_role_and_label(Role::CheckBox, "Gaming for CachyOS").scroll_to_me();
    h.run();
    h.get_by_role_and_label(Role::CheckBox, "Gaming for CachyOS").click();
    h.run();
    assert!(selection(&h));
    press(&mut h, "Review");
    assert_eq!(title(&h), "CachyOS · Minimal · no desktop");
}

#[test]
fn desktop_step_offers_composing_on_desktop_editions() {
    let mut h = harness(false);
    press(&mut h, "Next");
    press(&mut h, "Next");
    press(&mut h, "Fedora family");
    press(&mut h, "Fedora Linux");
    press(&mut h, "GNOME · Workstation");
    press(&mut h, "Next");
    assert_eq!(app(&h).step(), Step::Desktop);
    press(&mut h, "Or compose your own desktop on Fedora Linux Minimal");
    assert_eq!(app(&h).step(), Step::Desktop, "stays to pick the compositor and shell");
    assert!(!app(&h).can_continue(), "compose offer does not pick a compositor or shell");
    press(&mut h, "niri");
    assert!(!app(&h).can_continue());
    press(&mut h, "Noctalia");
    press(&mut h, "Review");
    assert_eq!(title(&h), "Fedora Linux · Minimal · niri + Noctalia");
}

#[test]
fn keyboard_moves_between_steps() {
    let mut h = harness(false);
    h.key_press(egui::Key::Enter);
    h.run();
    assert_eq!(app(&h).step(), Step::Machine);
    h.key_press(egui::Key::Escape);
    h.run();
    assert_eq!(app(&h).step(), Step::Connect);
}

/// Set WINDPAD_SHOTS to a directory to save screenshots for review.
#[test]
fn renders_each_step() {
    let Ok(dir) = std::env::var("WINDPAD_SHOTS") else { return };
    for (dark, name) in [(true, "dark"), (false, "light")] {
        let mut h = harness(dark);
        h.render().unwrap().save(format!("{dir}/live-{name}-1-connect.png")).unwrap();
        press(&mut h, "Next");
        h.render().unwrap().save(format!("{dir}/live-{name}-2-machine.png")).unwrap();
        press(&mut h, "Next");
        h.render().unwrap().save(format!("{dir}/live-{name}-3-family.png")).unwrap();
        press(&mut h, "Arch family");
        press(&mut h, "CachyOS");
        press(&mut h, "Minimal");
        h.render().unwrap().save(format!("{dir}/live-{name}-4-distribution.png")).unwrap();
        press(&mut h, "Next");
        press(&mut h, "Compose a desktop");
        press(&mut h, "MangoWC");
        press(&mut h, "No shell");
        h.render().unwrap().save(format!("{dir}/live-{name}-5-desktop.png")).unwrap();
        h.get_by_role_and_label(Role::CheckBox, "Gaming for CachyOS").scroll_to_me();
        h.run();
        h.render().unwrap().save(format!("{dir}/live-{name}-5-extras.png")).unwrap();
        press(&mut h, "Review");
        h.render().unwrap().save(format!("{dir}/live-{name}-6-review.png")).unwrap();
    }
}

fn with_network(online: bool) -> Installer {
    use borea_network::Connectivity;
    with_connectivity(if online { Connectivity::Full } else { Connectivity::None })
}

fn with_connectivity(connectivity: borea_network::Connectivity) -> Installer {
    use borea_network::{Band, Connectivity, Network, Security, Status};
    let status = Status {
        connectivity,
        radio_enabled: true,
        radio_unblocked: true,
        primary: (connectivity != Connectivity::None).then(|| "Breeze Studio".into()),
    };
    let networks = vec![
        Network::sample(
            "Breeze Studio",
            82,
            Security::Wpa3Personal,
            Band::Ghz5,
            connectivity != Connectivity::None,
        ),
        Network::sample("Neighbours", 40, Security::WpaPersonal, Band::Ghz2_4, false),
    ];
    let mut harness = Harness::builder().with_size(egui::vec2(1280.0, 860.0)).build_ui_state(
        move |ui, app: &mut Option<LiveInstaller>| {
            let ctx = ui.ctx().clone();
            let app = app.get_or_insert_with(|| {
                let link = borea_gui::wifi::WifiLink::simulated(status.clone(), networks.clone());
                LiveInstaller::with_link(&ctx, machine(), link, true)
            });
            for action in app.frame(ui) {
                app.apply(&ctx, action);
            }
        },
        None,
    );
    harness.run();
    harness
}

#[test]
fn online_at_boot_skips_straight_past_connect() {
    let h = with_network(true);
    assert_eq!(app(&h).step(), Step::Machine, "nobody should press Next to say they have a cable");
    assert!(h.query_by_label("Online").is_some(), "the rail records how it connected");
}

#[test]
fn joined_network_with_limited_internet_is_not_labelled_offline() {
    let mut h = with_connectivity(borea_network::Connectivity::Limited);
    assert_eq!(app(&h).step(), Step::Connect, "unconfirmed internet does not skip Connect");
    assert!(app(&h).suggested_network().is_none(), "do not prompt for an unrelated network");
    assert!(h.query_by_label("Passphrase").is_none(), "already joined; no new key required");
    press(&mut h, "Next");
    assert!(h.query_by_label("Connected · local network only").is_some());
    assert!(h.query_by_label("Not connected").is_none());
}

#[test]
fn offline_at_boot_has_the_strongest_network_ready_to_join() {
    let h = with_network(false);
    assert_eq!(app(&h).step(), Step::Connect);
    assert_eq!(app(&h).suggested_network(), Some("Breeze Studio"));
    assert!(h.get_by_label("Passphrase").is_focused(), "the cursor waits in the passphrase field");
    assert!(h.query_by_label("Join Breeze Studio").is_none(), "Join appears after typing");
}

#[test]
fn join_appears_beside_the_passphrase_after_typing() {
    let mut h = with_network(false);
    assert!(h.query_by_label("Join Breeze Studio").is_none());
    h.get_by_label("Passphrase").type_text("valid-wifi-passphrase");
    h.run();
    assert!(h.query_by_role_and_label(Role::Button, "Join Breeze Studio").is_some());
    let field = h.get_by_label("Passphrase").rect();
    let join = h.get_by_role_and_label(Role::Button, "Join Breeze Studio").rect();
    assert!(join.left() >= field.right(), "Join is to the right of the field");
    assert!((join.center().y - field.center().y).abs() < 8.0, "Join shares the input row");
    if let Ok(dir) = std::env::var("WINDPAD_SHOTS") {
        h.render().unwrap().save(format!("{dir}/live-wifi-join.png")).unwrap();
    }
    h.get_by_role_and_label(Role::Button, "Join Breeze Studio").click();
    h.run();
    assert!(h.query_by_label("Join Breeze Studio").is_none(), "submission clears the passphrase");
}

#[test]
fn secure_boot_is_off_unless_chosen_and_safe_to_choose() {
    let mut h = harness(false);
    press(&mut h, "Next");
    press(&mut h, "Next");
    press(&mut h, "Arch family");
    press(&mut h, "Arch Linux");
    press(&mut h, "KDE Plasma");
    press(&mut h, "Next");
    press(&mut h, "Review");
    assert!(!app(&h).selection().secure_boot, "off by default");
    assert!(
        h.query_by_label_contains("Keep Microsoft's keys").is_none(),
        "steps appear only when chosen"
    );

    h.get_by_role_and_label(Role::RadioButton, "Turn Secure Boot on").scroll_to_me();
    h.run();
    h.get_by_role_and_label(Role::RadioButton, "Turn Secure Boot on").click();
    h.run();
    assert!(app(&h).selection().secure_boot);
    assert!(
        h.query_by_label_contains("Keep Microsoft's keys").is_some(),
        "every step is shown before approval"
    );
    assert!(
        h.query_by_label_contains("enroll from firmware setup").is_some(),
        "no Setup Mode: manual enrollment explained"
    );

    // Bazzite's image-based boot path isn't certified with Secure Boot: the choice drops back to off.
    press(&mut h, "Step 3: Family");
    press(&mut h, "Fedora family");
    press(&mut h, "Bazzite");
    assert!(!app(&h).selection().secure_boot);
    press(&mut h, "KDE Plasma");
    press(&mut h, "Next");
    press(&mut h, "Review");
    h.get_by_role_and_label(Role::RadioButton, "Leave Secure Boot off").scroll_to_me();
    h.run();
    assert!(h.query_by_role_and_label(Role::RadioButton, "Leave Secure Boot off").is_some());
    assert!(h.query_by_label_contains("Install without Secure Boot").is_some());
}

#[test]
fn family_and_distribution_are_explicit_and_on_separate_pages() {
    let mut h = harness(false);
    assert!(h.query_by_label("Choose a family").is_some());
    press(&mut h, "Next");
    press(&mut h, "Next");
    assert!(!app(&h).can_continue());
    assert!(h.query_by_role_and_label(Role::Button, "Arch Linux").is_none());
    assert!(h.query_by_role_and_label(Role::Button, "None").is_none());
    press(&mut h, "Next");
    assert_eq!(app(&h).step(), Step::System);
    // Keyboard/action dispatch must enforce the same guard as the disabled button.
    h.state_mut().as_mut().unwrap().apply(&egui::Context::default(), Action::Next);
    h.run();
    assert_eq!(app(&h).step(), Step::System);

    for (family, distros) in [
        ("Arch family", &["Arch Linux", "CachyOS", "Omarchy"][..]),
        ("Fedora family", &["Fedora Linux", "Bazzite", "Nobara"]),
        ("Debian family", &["Debian", "The Butterbian Project", "PikaOS"]),
        ("openSUSE family", &["openSUSE Tumbleweed"]),
    ] {
        press(&mut h, family);
        assert_eq!(app(&h).step(), Step::Distribution);
        assert!(!app(&h).can_continue(), "{family} has no default distribution");
        assert!(h.query_by_role_and_label(Role::Button, "None").is_none());
        assert!(h.query_by_role_and_label(Role::Button, "Minimal").is_none());
        for distro in distros {
            assert!(h.query_by_role_and_label(Role::Button, distro).is_some(), "{distro}");
        }
        h.state_mut().as_mut().unwrap().apply(&egui::Context::default(), Action::Next);
        h.run();
        assert_eq!(app(&h).step(), Step::Distribution);
        if let Ok(dir) = std::env::var("WINDPAD_SHOTS") {
            h.render()
                .unwrap()
                .save(format!("{dir}/live-{}-distributions.png", family.split(' ').next().unwrap()))
                .unwrap();
        }
        press(&mut h, distros[0]);
        assert!(!app(&h).can_continue(), "no default edition, even for a sole distribution");
        press(&mut h, "Back");
        assert_eq!(app(&h).step(), Step::System);
    }
}

#[test]
fn variants_and_desktop_require_choices_and_invalidate_later_review() {
    let mut h = harness(false);
    press(&mut h, "Next");
    press(&mut h, "Next");
    press(&mut h, "Debian family");
    press(&mut h, "PikaOS");
    press(&mut h, "GNOME");
    assert!(!app(&h).can_continue(), "image variant is required");
    press(&mut h, "NVIDIA");
    assert!(app(&h).can_continue());
    press(&mut h, "Next");
    press(&mut h, "Review");
    press(&mut h, "Step 4: Distribution");
    press(&mut h, "KDE Plasma");
    assert!(!app(&h).can_continue(), "new edition needs a variant decision");
    press(&mut h, "Step 6: Review");
    assert_eq!(app(&h).step(), Step::Distribution);

    press(&mut h, "Step 3: Family");
    press(&mut h, "Arch family");
    press(&mut h, "Arch Linux");
    press(&mut h, "Minimal");
    press(&mut h, "Next");
    assert!(!app(&h).can_continue(), "no default desktop mode");
    press(&mut h, "Compose a desktop");
    assert!(!app(&h).can_continue(), "no default compositor");
    press(&mut h, "Hyprland");
    assert!(!app(&h).can_continue(), "no default shell");
    h.state_mut().as_mut().unwrap().apply(&egui::Context::default(), Action::Next);
    h.run();
    assert_eq!(app(&h).step(), Step::Desktop);
    press(&mut h, "No shell");
    press(&mut h, "Review");
    press(&mut h, "Step 5: Desktop");
    press(&mut h, "niri");
    assert!(!app(&h).can_continue(), "changing compositor invalidates the shell decision");
    press(&mut h, "Step 6: Review");
    assert_eq!(app(&h).step(), Step::Desktop);
}

#[test]
fn butterbian_editions_are_visible_but_upstream_is_unverified() {
    let mut h = harness(false);
    press(&mut h, "Next");
    press(&mut h, "Next");
    press(&mut h, "Debian family");
    press(&mut h, "The Butterbian Project");
    assert!(h.query_by_label_contains("Upstream source pending verification.").is_some());
    assert!(h.query_by_role_and_label(Role::Button, "Xfce").is_some());
    assert!(h.query_by_role_and_label(Role::Button, "mangobutter").is_some());
    assert!(!app(&h).can_continue());
    if let Ok(dir) = std::env::var("WINDPAD_SHOTS") {
        h.render().unwrap().save(format!("{dir}/live-butterbian-editions.png")).unwrap();
    }
    press(&mut h, "mangobutter");
    press(&mut h, "Next");
    assert!(h.query_by_label_contains("Extra packages will be offered after").is_some());
    press(&mut h, "Review");
    assert_eq!(title(&h), "The Butterbian Project · mangobutter");
    assert!(!app(&h).can_continue(), "catalog presence is not an executable installer");
}

#[test]
fn opensuse_offers_opt_in_extra_packages() {
    let mut h = harness(false);
    press(&mut h, "Next");
    press(&mut h, "Next");
    press(&mut h, "openSUSE family");
    press(&mut h, "openSUSE Tumbleweed");
    press(&mut h, "Xfce");
    press(&mut h, "Next");
    for extra in ["Gaming", "Media codecs", "Flatpak", "Printing"] {
        assert!(
            h.query_by_role_and_label(Role::CheckBox, &format!("{extra} for openSUSE Tumbleweed"))
                .is_some()
        );
    }
    assert!(app(&h).selection().ticked(app(&h).catalog(), Family::Opensuse).is_empty());
    h.get_by_role_and_label(Role::CheckBox, "Flatpak for openSUSE Tumbleweed").scroll_to_me();
    h.run();
    h.get_by_role_and_label(Role::CheckBox, "Flatpak for openSUSE Tumbleweed").click();
    h.run();
    press(&mut h, "Review");
    assert!(
        app(&h)
            .selection()
            .rows(app(&h).catalog(), Family::Opensuse)
            .contains(&("Extras", "Flatpak".into()))
    );
}

#[test]
fn short_window_keeps_navigation_reachable() {
    let mut h = harness(false);
    h.set_size(egui::vec2(900.0, 600.0));
    h.run();
    press(&mut h, "Next");
    press(&mut h, "Next");
    press(&mut h, "Arch family");
    press(&mut h, "Omarchy");
    assert!(!app(&h).can_continue(), "even a sole edition must be picked");
    press(&mut h, "Hyprland · Omarchy");
    press(&mut h, "Next");
    press(&mut h, "Review");
    assert_eq!(app(&h).step(), Step::Review);
    press(&mut h, "Step 3: Family");
    assert_eq!(app(&h).step(), Step::System);
    if let Ok(dir) = std::env::var("WINDPAD_SHOTS") {
        h.render().unwrap().save(format!("{dir}/live-small-family.png")).unwrap();
    }
}

/// Choosing Omarchy installs it: pick where it goes (nothing preselected), check
/// the plan with the service, confirm shrinking Windows, begin, and the hand-off
/// runs with exactly the reviewed plan and its approval.
#[test]
fn omarchy_hands_off_to_its_own_installer_as_chosen() {
    use serde_json::{Value, json};
    use std::sync::{Arc, Mutex};
    const GIB: u64 = 1 << 30;
    let calls = Arc::new(Mutex::new(Vec::<(String, Value)>::new()));
    let seen = Arc::clone(&calls);
    let service: borea_gui::handoff::Service = Arc::new(move |method, params| {
        seen.lock().unwrap().push((method.into(), params.clone()));
        Ok(match method {
            "disks.list" => json!({"uefi":true,"disks":[
                {"id":{"logical_sector":512,"path":"/dev/sda","serial":"STICK","size":32_000_000_000_u64,"model":"Borea stick"},
                 "readonly":false,"in_use":true,"live":true,"parts":[]},
                {"id":{"logical_sector":512,"path":"/dev/nvme0n1","serial":"S6P7","size":512_000_000_000_u64,"model":"Samsung 980"},
                 "readonly":false,"in_use":false,"live":false,"parts":[
                    {"path":"/dev/nvme0n1p3","number":3,"uuid":"WIN","fs":"ntfs","max_linux_size":200*GIB}]}]}),
            "handoff.review" => json!({"approval":"feed","review":{
                "steps":["Download Omarchy 4.0.6 to the Borea stick","Shrink Windows on Samsung 980 by 60 GiB"],
                "choose":"In Omarchy's installer, choose Samsung 980 and \"Free space install\"."}}),
            "handoff.start" => json!({"job_id":"feed"}),
            _ => json!({"events":[
                {"event":"stage-started","stage":0,"title":"Downloading Omarchy 4.0.6."},
                {"event":"finished"}],"cursor":2,"more":false}),
        })
    });
    let mut h = harness(false);
    h.state_mut().as_mut().unwrap().set_handoff_service(service);
    press(&mut h, "Next");
    press(&mut h, "Next");
    press(&mut h, "Arch family");
    assert!(
        h.query_by_label_contains("Official installer, via Borea").is_some(),
        "Omarchy is not shown as planned"
    );
    if let Ok(dir) = std::env::var("WINDPAD_SHOTS") {
        h.render().unwrap().save(format!("{dir}/live-arch-distributions.png")).unwrap();
    }
    press(&mut h, "Omarchy");
    press(&mut h, "Hyprland · Omarchy");
    press(&mut h, "Next");
    press(&mut h, "Review");
    let settle = |h: &mut Installer| {
        for _ in 0..60 {
            h.run();
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    };
    settle(&mut h);
    assert!(h.query_by_label_contains("Preview build").is_none(), "Omarchy can install");
    assert!(h.query_by_label_contains("Borea stick").is_none(), "never the live stick");
    assert!(!app(&h).can_continue(), "nothing is preselected");
    press(&mut h, "Beside Windows on Samsung 980 · 512 GB");
    press(&mut h, "Check this plan");
    settle(&mut h);
    assert!(h.query_by_label_contains("Shrink Windows on Samsung 980 by 60 GiB").is_some());
    assert!(!app(&h).can_continue(), "shrinking Windows needs a confirmation");
    let confirm = "Shrink Windows now. I have a backup of anything important on it.";
    h.get_by_role_and_label(Role::CheckBox, confirm).scroll_to_me();
    h.run();
    h.get_by_role_and_label(Role::CheckBox, confirm).click();
    h.run();
    assert!(app(&h).can_continue());
    assert!(h.query_by_label_contains("Once you begin, leave this computer alone.").is_some());
    if let Ok(dir) = std::env::var("WINDPAD_SHOTS") {
        h.render().unwrap().save(format!("{dir}/live-omarchy-review.png")).unwrap();
    }
    press(&mut h, "Begin installation");
    settle(&mut h);
    assert!(h.query_all_by_label_contains("Keep the Borea stick in").next().is_some());
    if let Ok(dir) = std::env::var("WINDPAD_SHOTS") {
        h.render().unwrap().save(format!("{dir}/live-omarchy-restarting.png")).unwrap();
    }
    assert!(
        matches!(app(&h).handoff().phase, borea_gui::handoff::Phase::Finished { .. }),
        "{:?}",
        app(&h).handoff().phase
    );
    assert!(h.query_by_label_contains("choose Samsung 980 and \"Free space install\"").is_some());
    let calls = calls.lock().unwrap();
    let start = &calls.iter().find(|(m, _)| m == "handoff.start").unwrap().1;
    assert_eq!(start["approval"], "feed");
    assert_eq!(start["handoff"]["target"], "omarchy");
    assert_eq!(start["handoff"]["disk"]["kind"], "alongside");
    assert_eq!(start["handoff"]["disk"]["linux_size"], 60 * GIB);
    assert_eq!(start["handoff"]["disk"]["disk"]["serial"], "S6P7");
}
