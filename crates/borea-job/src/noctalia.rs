//! Target configuration for graphical password login, with no TTY setup step.
use super::{Action, target, write};

pub fn configure(plan: &borea_core::Plan) -> Vec<Action> {
    let Some(recipe) = plan.desktop.and_then(borea_core::Desktop::noctalia) else {
        return vec![];
    };
    let keyboard = &plan.locale.keyboard;
    // Layout and variant are checked against the system's XKB catalog by installd.
    let layout = serde_json::to_string(&keyboard.layout).unwrap_or_default();
    let variant = serde_json::to_string(&keyboard.variant).unwrap_or_default();
    let lang = serde_json::to_string(&plan.locale.lang).unwrap_or_default();
    let hyprland = format!(
        "{HYPRLAND}\nhl.config({{ input = {{ kb_layout = {layout}, kb_variant = {variant} }} }})\n"
    );
    let mut actions = vec![
        write("/etc/greetd/config.toml", "[terminal]\nvt = 1\nswitch = true\n\n[default_session]\ncommand = \"/usr/bin/noctalia-greeter-session\"\nuser = \"greeter\"\n".into(), 0o644),
        target("/usr/bin/systemd-tmpfiles", ["--create", "noctalia-greeter.conf"]),
        target("/usr/bin/noctalia-greeter-apply-appearance", ["--setup-system"]),
        // The greeter is its own Wayland compositor: a console KEYMAP alone
        // cannot set its XKB layout. Apply the reviewed choices before login.
        write("/var/lib/noctalia-greeter/greeter.toml", format!("[keyboard]\nlayout = {layout}\nvariant = {variant}\n"), 0o644),
        target("/usr/bin/systemctl", ["mask", "getty@tty1.service"]),
        // Noctalia accepts POSIX locale names and handles translation fallback.
        // Omit [keybinds] so its in-shell navigation retains upstream defaults.
        write("/etc/skel/.config/noctalia/config.toml", format!("[shell]\nlang = {lang}\n\n[lockscreen]\nenabled = true\nlock_before_suspend = true\ngrace_period_seconds = 0\n\n[idle.behavior.lock]\ntimeout = 600\naction = \"lock\"\nenabled = true\n"), 0o644),
        write("/etc/systemd/system/borea-firstboot.service.d/login.conf", format!("[Service]\nExecStart=\nExecStart=/usr/bin/borea-firstboot --after-login {}\nNice=10\nIOSchedulingClass=best-effort\nIOSchedulingPriority=7\n", plan.user.username), 0o644),
        write("/etc/plymouth/plymouthd.conf", "[Daemon]\nTheme=spinner\nShowDelay=0\n".into(), 0o644),
    ];
    let exec = match recipe.compositor {
        "hyprland" | "niri" => recipe.session.to_owned(),
        _ => format!(
            "/usr/bin/env XDG_CURRENT_DESKTOP={} XDG_SESSION_DESKTOP={} XDG_SESSION_TYPE=wayland /usr/bin/dbus-run-session -- {}",
            recipe.desktop_name, recipe.compositor, recipe.session
        ),
    };
    actions.push(write(&format!("/usr/share/wayland-sessions/borea-{}.desktop",recipe.compositor),
        format!("[Desktop Entry]\nName={} + Noctalia\nComment={} with Noctalia v5\nExec={exec}\nTryExec={}\nType=Application\nDesktopNames={}\n",recipe.name,recipe.name,recipe.session,recipe.desktop_name),0o644));
    actions.push(write("/etc/skel/.config/xdg-desktop-portal/portals.conf",format!("[preferred]\ndefault={};gtk;\norg.freedesktop.impl.portal.FileChooser=gtk\norg.freedesktop.impl.portal.ScreenCast={}\n",recipe.portal,recipe.portal),0o644));
    match recipe.compositor {
        "hyprland" => actions.push(write("/etc/skel/.config/hypr/hyprland.lua", hyprland, 0o644)),
        "niri" => actions.push(write(
            "/etc/skel/.config/niri/config.kdl",
            NIRI.replace("@@LAYOUT@@", &layout).replace("@@VARIANT@@", &variant),
            0o644,
        )),
        "mango" => actions.push(write(
            "/etc/skel/.config/mango/config.conf",
            MANGO.replace("@@LAYOUT@@", &keyboard.layout).replace("@@VARIANT@@", &keyboard.variant),
            0o644,
        )),
        _ => {}
    }
    actions
}
const NIRI: &str = include_str!("../data/noctalia/niri.kdl");
const MANGO: &str = include_str!("../data/noctalia/mango.conf");

// Hyprland 0.57 uses Lua. Noctalia handles logind's sleep-delay inhibitor itself,
// waiting for lock surfaces before releasing it; do not add a competing locker.
const HYPRLAND: &str = r#"-- Borea: upstream Hyprland with Noctalia v5
hl.monitor({ output = "", mode = "preferred", position = "auto", scale = "auto" })
hl.env("XDG_CURRENT_DESKTOP", "Hyprland")
hl.env("XDG_SESSION_DESKTOP", "Hyprland")
hl.env("XDG_SESSION_TYPE", "wayland")
hl.on("hyprland.start", function()
    hl.exec_cmd("dbus-update-activation-environment --systemd WAYLAND_DISPLAY XDG_CURRENT_DESKTOP XDG_SESSION_TYPE")
    hl.exec_cmd("noctalia")
end)
-- Noctalia v5's documented Hyprland bindings:
-- https://github.com/noctalia-dev/noctalia/blob/v5.4.1/docs/user/compositor-settings/hyprland.mdx
hl.bind("SUPER + SPACE", hl.dsp.exec_cmd("noctalia msg panel-toggle launcher"))
hl.bind("SUPER + S", hl.dsp.exec_cmd("noctalia msg panel-toggle control-center"))
hl.bind("SUPER + comma", hl.dsp.exec_cmd("noctalia msg settings-toggle"))
hl.bind("ALT + Tab", hl.dsp.exec_cmd("noctalia msg window-switcher hold"))
hl.bind("XF86AudioRaiseVolume", hl.dsp.exec_cmd("noctalia msg volume-up"))
hl.bind("XF86AudioLowerVolume", hl.dsp.exec_cmd("noctalia msg volume-down"))
hl.bind("XF86AudioMute", hl.dsp.exec_cmd("noctalia msg volume-mute"))
hl.bind("XF86MonBrightnessUp", hl.dsp.exec_cmd("noctalia msg brightness-up"))
hl.bind("XF86MonBrightnessDown", hl.dsp.exec_cmd("noctalia msg brightness-down"))

-- Hyprland's default window/workspace keys, with the installed terminal and
-- Noctalia launcher/session actions. Noctalia owns Super+S (Control Center).
-- https://github.com/hyprwm/Hyprland/blob/v0.57.0/example/hyprland.lua
hl.bind("SUPER + Q", hl.dsp.exec_cmd("foot"))
hl.bind("SUPER + C", hl.dsp.window.close())
hl.bind("SUPER + M", hl.dsp.exec_cmd("noctalia msg session logout"))
hl.bind("SUPER + R", hl.dsp.exec_cmd("noctalia msg panel-toggle launcher"))
hl.bind("SUPER + V", hl.dsp.window.float({ action = "toggle" }))
hl.bind("SUPER + P", hl.dsp.window.pseudo())
hl.bind("SUPER + J", hl.dsp.layout("togglesplit"))
for _, direction in ipairs({ "left", "right", "up", "down" }) do
    hl.bind("SUPER + " .. direction, hl.dsp.focus({ direction = direction }))
end
hl.bind("SUPER + mouse_down", hl.dsp.focus({ workspace = "e+1" }))
hl.bind("SUPER + mouse_up", hl.dsp.focus({ workspace = "e-1" }))
hl.bind("SUPER + mouse:272", hl.dsp.window.drag(), { mouse = true })
hl.bind("SUPER + mouse:273", hl.dsp.window.resize(), { mouse = true })
for i = 1, 10 do
    local key = i % 10
    hl.bind("SUPER + " .. key, hl.dsp.focus({ workspace = i }))
    hl.bind("SUPER + SHIFT + " .. key, hl.dsp.window.move({ workspace = i }))
end
"#;
