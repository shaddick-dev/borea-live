//! Extras for a hand-off target, added quietly on the installed system's first
//! start. The target's own installer has no hook for extra packages, and its ISO
//! is booted as published (signature-checked), so Borea adds nothing to it.
//! Borea controls only what it boots the installer with, and the stick:
//!
//! 1. Borea writes the chosen packages and two small scripts to the stick's data
//!    partition (`borea/extras/`), which the installer's live system has mounted
//!    (it boots from the ISO file there) at `/run/archiso/img_dev`.
//! 2. The installer's kernel command line carries one systemd unit, as a system
//!    credential (`systemd.extra-unit.*`, read by systemd-debug-generator) that
//!    `systemd.wants=` starts. It waits for the installer to finish, then copies
//!    a first-start service into the installed system.
//! 3. On the installed system's first start, that service installs the packages
//!    from Arch's repositories (online), turns on multilib when an extra needs
//!    it, and removes itself. Offline or failed, it tries again next start.

use crate::upstream::Extra;

/// Where the files go on the data partition, and where the live system sees them.
pub const DIR: &str = "borea/extras";
pub const LIVE_DIR: &str = "/run/archiso/img_dev/borea/extras";
pub const STAGE_UNIT: &str = "borea-extras-stage.service";

/// Runs in the installer's live system: waits for the install, then stages the
/// first-start service into the installed system.
pub const STAGE: &str = r#"#!/bin/bash
# Borea: stage the chosen extras into the system Omarchy's installer installs.
# Runs until the install finishes; with --now (the unit's ExecStop, as the live
# system shuts down or restarts) it stages at once if the install is done, so a
# restart straight after the install still carries the extras.
set -uo pipefail
src=$(dirname "$(readlink -f "$0")")
state=/run/omarchy-install/state.json
done_flag=/run/borea-extras-staged
finished() { [[ -f $state ]] && jq -e '.finished_at != null' "$state" >/dev/null 2>&1; }
stage() {
  [[ -e $done_flag ]] && return 0
  local target lib
  target=$(jq -r '.target // "/mnt"' "$state")
  if [[ ! -d $target/etc/systemd/system ]]; then
    echo "borea-extras: no installed system at $target; nothing staged"
    return 1
  fi
  lib=$target/usr/local/lib/borea/extras
  install -d -m 755 "$lib" "$target/etc/systemd/system/multi-user.target.wants"
  install -m 644 "$src/packages" "$lib/packages"
  for flag in multilib poweroff-when-done; do
    [[ -f $src/$flag ]] && install -m 644 "$src/$flag" "$lib/$flag"
  done
  install -m 755 "$src/install.sh" "$lib/install.sh"
  install -m 644 "$src/borea-extras.service" "$target/etc/systemd/system/borea-extras.service"
  ln -sf ../borea-extras.service "$target/etc/systemd/system/multi-user.target.wants/borea-extras.service"
  sync
  touch "$done_flag"
  echo "borea-extras: staged for the first start: $(tr '\n' ' ' <"$src/packages")"
}
if [[ ${1:-} == --now ]]; then
  finished && stage
  exit 0
fi
echo "borea-extras: waiting for the installer to finish"
until finished; do sleep 1; done
stage
"#;

/// Runs on the installed system's first start (and again until it succeeds).
pub const INSTALL: &str = r#"#!/bin/bash
# Borea: add the extras chosen in Borea Live, then remove this service.
set -uo pipefail
lib=/usr/local/lib/borea/extras
mapfile -t packages <"$lib/packages"
# Online first: up to ten minutes for a connection (Wi-Fi may still be joining).
for _ in $(seq 120); do
  curl -fsI --max-time 5 https://archlinux.org >/dev/null 2>&1 && break
  sleep 5
done
# Never alongside another pacman (the first start may be updating too).
while [[ -e /var/lib/pacman/db.lck ]]; do sleep 5; done
if [[ -f $lib/multilib ]]; then
  if ! grep -q '^\[multilib\]' /etc/pacman.conf; then
    if grep -q '^#\[multilib\]' /etc/pacman.conf; then
      sed -i '/^#\[multilib\]/{s/^#//;n;s/^#//}' /etc/pacman.conf
    else
      printf '\n[multilib]\nInclude = /etc/pacman.d/mirrorlist\n' >>/etc/pacman.conf
    fi
  fi
  # 32-bit Vulkan for this machine's graphics, so pacman picks no stranger.
  for dev in /sys/bus/pci/devices/*; do
    [[ $(cat "$dev/class" 2>/dev/null) == 0x03* ]] || continue
    case $(cat "$dev/vendor" 2>/dev/null) in
      0x8086) packages+=(lib32-vulkan-intel) ;;
      0x1002) packages+=(lib32-vulkan-radeon) ;;
      0x10de) packages+=(lib32-nvidia-utils) ;;
    esac
  done
fi
echo "borea-extras: adding ${packages[*]}"
# A full sync with the upgrade, as Arch requires: never a partial upgrade.
if pacman -Syu --noconfirm --needed "${packages[@]}"; then
  echo "borea-extras: done"
  systemctl disable borea-extras.service
  rm -f /etc/systemd/system/borea-extras.service
  poweroff=0
  [[ -f $lib/poweroff-when-done ]] && poweroff=1
  rm -rf "$lib"
  ((poweroff)) && systemctl poweroff --no-block
  exit 0
fi
echo "borea-extras: could not add them now; trying again at the next start"
exit 1
"#;

/// The installed system's first-start service.
pub const SERVICE: &str = "[Unit]
Description=Borea: add the extras chosen in Borea Live
Wants=network-online.target
After=network-online.target
ConditionPathExists=/usr/local/lib/borea/extras/packages

[Service]
Type=oneshot
ExecStart=/usr/local/lib/borea/extras/install.sh
Nice=10
TimeoutStartSec=3h

[Install]
WantedBy=multi-user.target
";

/// The live-system unit, small enough for the kernel command line.
pub fn stage_unit() -> String {
    format!(
        "[Unit]\nDescription=Borea: stage the chosen extras\n\n[Service]\nExecStart=/bin/bash {LIVE_DIR}/stage.sh\nExecStop=/bin/bash {LIVE_DIR}/stage.sh --now\n"
    )
}

/// Standard base64 with padding, as systemd.set_credential_binary= reads it.
fn base64(bytes: &[u8]) -> String {
    const ABC: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [chunk[0], *chunk.get(1).unwrap_or(&0), *chunk.get(2).unwrap_or(&0)];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(char::from(ABC[((n >> (18 - 6 * i)) & 63) as usize]));
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// What to add to the installer's kernel command line.
pub fn cmdline() -> String {
    let unit = base64(stage_unit().as_bytes());
    format!(
        "systemd.set_credential_binary=systemd.extra-unit.{STAGE_UNIT}:{unit} systemd.wants={STAGE_UNIT}"
    )
}

/// The files to write to `borea/extras/` on the data partition, as (name,
/// contents). `poweroff` asks the installed system to power off once done (the
/// VM test only).
pub fn files(extras: &[&Extra], poweroff: bool) -> Vec<(&'static str, String)> {
    let mut packages: Vec<&str> = Vec::new();
    for p in extras.iter().flat_map(|e| e.packages.iter()) {
        if !packages.contains(p) {
            packages.push(p);
        }
    }
    let mut out = vec![
        ("packages", packages.join("\n") + "\n"),
        ("stage.sh", STAGE.to_owned()),
        ("install.sh", INSTALL.to_owned()),
        ("borea-extras.service", SERVICE.to_owned()),
    ];
    if extras.iter().any(|e| e.multilib) {
        out.push(("multilib", "yes\n".to_owned()));
    }
    if poweroff {
        out.push(("poweroff-when-done", "yes\n".to_owned()));
    }
    out
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic)]
mod tests {
    use super::*;
    use crate::upstream::OMARCHY;

    #[test]
    fn the_command_line_carries_one_small_unit() {
        let line = cmdline();
        assert!(line.contains("systemd.wants=borea-extras-stage.service"));
        // Omarchy's own options take about 250 of the 2048 bytes x86 allows.
        assert!(line.len() < 600, "{}", line.len());
        assert!(line.contains(&base64(stage_unit().as_bytes())));
        assert!(!line.contains(char::is_control));
    }

    #[test]
    fn base64_is_standard() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
        assert_eq!(base64(&[0xfb, 0xff]), "+/8=");
    }

    #[test]
    fn files_list_each_package_once_and_flag_multilib_only_when_needed() {
        let gaming = OMARCHY.extra("gaming").unwrap();
        let printing = OMARCHY.extra("printing").unwrap();
        let files = files(&[printing, gaming, printing], false);
        let get = |n: &str| files.iter().find(|(name, _)| *name == n).map(|(_, c)| c.clone());
        let packages = get("packages").unwrap();
        assert_eq!(packages.lines().filter(|l| *l == "cups").count(), 1);
        assert!(packages.lines().any(|l| l == "steam"));
        assert!(get("multilib").is_some() && get("poweroff-when-done").is_none());
        let plain = super::files(&[printing], true);
        assert!(!plain.iter().any(|(n, _)| *n == "multilib"));
        assert!(plain.iter().any(|(n, _)| *n == "poweroff-when-done"));
    }

    #[test]
    fn package_names_are_plain_words() {
        for extra in OMARCHY.extras {
            for p in extra.packages {
                assert!(p.chars().all(|c| c.is_ascii_alphanumeric() || "-_.+".contains(c)), "{p}");
            }
        }
    }

    #[test]
    fn scripts_parse_as_bash() {
        let dir = std::env::temp_dir().join(format!("borea-extras-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        for (name, script) in [("stage.sh", STAGE), ("install.sh", INSTALL)] {
            let path = dir.join(name);
            std::fs::write(&path, script).unwrap();
            let ok = std::process::Command::new("bash").arg("-n").arg(&path).status();
            if let Ok(status) = ok {
                assert!(status.success(), "{name} has a syntax error");
            }
        }
        let _ = std::fs::remove_dir_all(dir);
    }
}
