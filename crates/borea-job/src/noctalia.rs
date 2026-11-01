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
        write("/etc/skel/.config/noctalia/config.toml", format!("[shell]\nlang = {lang}\n\n[lockscreen]\nenabled = true\nlock_before_suspend = true\ngrace_period_seconds = 0\n\n[idle.behavior.lock]\ntimeout = 600\naction = \"lock\"\nenabled = true\n{}", if recipe.compositor == "mango" { MANGO_SHADOWS } else { "" }), 0o644),
        write("/etc/systemd/system/borea-firstboot.service.d/login.conf", format!("[Service]\nExecStart=\nExecStart=/usr/bin/borea-firstboot --after-login {}\nNice=10\nIOSchedulingClass=best-effort\nIOSchedulingPriority=7\n", plan.user.username), 0o644),
        write("/etc/plymouth/plymouthd.conf", "[Daemon]\nTheme=spinner\nShowDelay=0\n".into(), 0o644),
    ];
    let exec = match recipe.compositor {
        "hyprland" | "niri" => recipe.session.to_owned(),
        // Keep greetd/PAM's user bus. A second dbus-run-session would isolate
        // applications from the systemd-activated portal services.
        _ => format!(
            "/usr/bin/env XDG_CURRENT_DESKTOP={} XDG_SESSION_DESKTOP={} XDG_SESSION_TYPE=wayland {}",
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
/// Noctalia's Mango guide: Mango draws the shadows, so Noctalia's are off.
const MANGO_SHADOWS: &str = "\n[bar.default]\nshadow = false\ncontact_shadow = false\n\n[dock]\nshadow = false\n\n[shell.panel]\nshadow = false\n";
const NIRI: &str = include_str!("../data/noctalia/niri.kdl");
const MANGO: &str = include_str!("../data/noctalia/mango.conf");

// Hyprland reads Lua from 0.56. Noctalia handles logind's sleep-delay inhibitor
// itself, waiting for lock surfaces before releasing it; no competing locker.
const HYPRLAND: &str = r#"-- Borea: Hyprland with Noctalia v5, set up as Noctalia's Hyprland guide describes:
-- https://github.com/noctalia-dev/noctalia/blob/v5.4.1/docs/user/compositor-settings/hyprland.mdx
-- Window keys are a small set shared by every Borea compositor.
hl.monitor({ output = "", mode = "preferred", position = "auto", scale = "auto" })
hl.env("XDG_CURRENT_DESKTOP", "Hyprland")
hl.env("XDG_SESSION_DESKTOP", "Hyprland")
hl.env("XDG_SESSION_TYPE", "wayland")
hl.on("hyprland.start", function()
    hl.exec_cmd("dbus-update-activation-environment --systemd WAYLAND_DISPLAY XDG_CURRENT_DESKTOP XDG_SESSION_TYPE")
    hl.exec_cmd("noctalia")
end)

-- Noctalia: window appearance.
hl.config({
    general = {
        gaps_in = 5,
        gaps_out = 10,
    },
    decoration = {
        rounding = 20,
        rounding_power = 2,
        shadow = {
            enabled = true,
            range = 4,
            render_power = 3,
            color = 0xee1a1a1a,
        },
        blur = {
            enabled = true,
            xray = true,
            size = 3,
            passes = 2,
            vibrancy = 0.1696,
        },
    },
})

-- Noctalia: persistent workspaces, so its indicator shows empty ones too.
for i = 1, 5 do
    hl.workspace_rule({ workspace = tostring(i), persistent = true })
end

-- Noctalia: its settings window floats.
hl.window_rule({
    match = { class = "dev.noctalia.Noctalia" },
    float = true,
    size = { 1080, 920 },
})

-- Noctalia: blur behind its surfaces, with its own animations.
hl.layer_rule({
    name = "noctalia",
    match = {
        namespace = "^noctalia-(bar-.+|notification|dock|panel|attached-panel|osd|window-switcher)$",
    },
    no_anim = true,
    ignore_alpha = 0.5,
    blur = true,
    blur_popups = true,
})

-- Noctalia
local ipc = "noctalia msg "
hl.bind("SUPER + SPACE", hl.dsp.exec_cmd(ipc .. "panel-toggle launcher"))
hl.bind("SUPER + S", hl.dsp.exec_cmd(ipc .. "panel-toggle control-center"))
hl.bind("SUPER + comma", hl.dsp.exec_cmd(ipc .. "settings-toggle"))
hl.bind("ALT + Tab", hl.dsp.exec_cmd(ipc .. "window-switcher hold"))
hl.bind("XF86AudioRaiseVolume", hl.dsp.exec_cmd(ipc .. "volume-up"))
hl.bind("XF86AudioLowerVolume", hl.dsp.exec_cmd(ipc .. "volume-down"))
hl.bind("XF86AudioMute", hl.dsp.exec_cmd(ipc .. "volume-mute"))
hl.bind("XF86MonBrightnessUp", hl.dsp.exec_cmd(ipc .. "brightness-up"))
hl.bind("XF86MonBrightnessDown", hl.dsp.exec_cmd(ipc .. "brightness-down"))

-- Windows
hl.bind("SUPER + T", hl.dsp.exec_cmd("foot"))
hl.bind("SUPER + Q", hl.dsp.window.close())
hl.bind("SUPER + V", hl.dsp.window.float({ action = "toggle" }))
hl.bind("SUPER + SHIFT + E", hl.dsp.exec_cmd(ipc .. "session logout"))
for _, direction in ipairs({ "left", "right", "up", "down" }) do
    hl.bind("SUPER + " .. direction, hl.dsp.focus({ direction = direction }))
end
hl.bind("SUPER + mouse:272", hl.dsp.window.drag(), { mouse = true })
hl.bind("SUPER + mouse:273", hl.dsp.window.resize(), { mouse = true })
for i = 1, 9 do
    hl.bind("SUPER + " .. i, hl.dsp.focus({ workspace = i }))
    hl.bind("SUPER + SHIFT + " .. i, hl.dsp.window.move({ workspace = i }))
end
"#;
