//! Target configuration for graphical password login, with no TTY setup step.
use super::{Action, target, write};

pub fn configure(plan: &borea_core::Plan) -> Vec<Action> {
    let keyboard = &plan.locale.keyboard;
    // Layout and variant are checked against the system's XKB catalog by installd.
    let layout = serde_json::to_string(&keyboard.layout).unwrap_or_default();
    let variant = serde_json::to_string(&keyboard.variant).unwrap_or_default();
    let hyprland = format!(
        "{HYPRLAND}\nhl.config({{ input = {{ kb_layout = {layout}, kb_variant = {variant} }} }})\n"
    );
    vec![
        write("/etc/greetd/config.toml", "[terminal]\nvt = 1\nswitch = true\n\n[default_session]\ncommand = \"/usr/bin/noctalia-greeter-session\"\nuser = \"greeter\"\n".into(), 0o644),
        target("/usr/bin/systemd-tmpfiles", ["--create", "noctalia-greeter.conf"]),
        target("/usr/bin/noctalia-greeter-apply-appearance", ["--setup-system"]),
        target("/usr/bin/systemctl", ["mask", "getty@tty1.service"]),
        write("/usr/share/wayland-sessions/borea-hyprland.desktop", "[Desktop Entry]\nName=Hyprland + Noctalia\nComment=Hyprland with Noctalia v5\nExec=/usr/bin/start-hyprland\nTryExec=/usr/bin/start-hyprland\nType=Application\nDesktopNames=Hyprland\n".into(), 0o644),
        write("/etc/skel/.config/hypr/hyprland.lua", hyprland, 0o644),
        write("/etc/skel/.config/noctalia/config.toml", "[lockscreen]\nenabled = true\nlock_before_suspend = true\ngrace_period_seconds = 0\n\n[idle.behavior.lock]\ntimeout = 600\naction = \"lock\"\nenabled = true\n".into(), 0o644),
        write("/etc/systemd/system/borea-firstboot.service.d/login.conf", format!("[Service]\nExecStart=\nExecStart=/usr/bin/borea-firstboot --after-login {}\nNice=10\nIOSchedulingClass=best-effort\nIOSchedulingPriority=7\n", plan.user.username), 0o644),
        write("/etc/plymouth/plymouthd.conf", "[Daemon]\nTheme=spinner\nShowDelay=0\n".into(), 0o644),
    ]
}

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
hl.bind("SUPER + RETURN", hl.dsp.exec_cmd("foot"))
hl.bind("SUPER + SPACE", hl.dsp.exec_cmd("noctalia msg launcher toggle"))
hl.bind("SUPER + L", hl.dsp.exec_cmd("noctalia msg session lock"))
hl.bind("SUPER + SHIFT + S", hl.dsp.exec_cmd("noctalia msg session lock-and-suspend"))
hl.bind("SUPER + SHIFT + Q", hl.dsp.window.close())
hl.bind("SUPER + SHIFT + E", hl.dsp.exec_cmd("noctalia msg session logout"))
hl.bind("SUPER + mouse:272", hl.dsp.window.drag(), { mouse = true })
hl.bind("SUPER + mouse:273", hl.dsp.window.resize(), { mouse = true })
for i = 1, 9 do
    hl.bind("SUPER + " .. i, hl.dsp.focus({ workspace = i }))
    hl.bind("SUPER + SHIFT + " .. i, hl.dsp.window.move({ workspace = i }))
end
"#;
