# Borea — automatic drivers (decision record, Oct 2026)

Drivers are never a question. The live ISO probes the machine it booted on and `plan_drivers` (`src/drivers.rs`) picks packages per distro.

## When and how it probes
- `borea-probe.service` (dist/) runs once per boot, after udev settles (so the live session's GPU drivers are loaded and `/sys/class/drm` shows which GPU drives the screen), before the installer UI.
- `borea-probe --write` (`src/probe.rs`, `src/bin/borea-probe.rs`) scans `/sys`, `/proc` and hwdata's `pci.ids`, then writes `/run/borea/hardware.json`: the machine, its GPUs, and a driver plan for every distro. The installer only reads that file; choosing a distro on page 03 just picks which plan applies.
- No model is special-cased. The ASUS TUF Gaming F15 and the other machines in `hardware::samples()` are fixtures for tests and the design prototype's `hw` switch only.
- `borea-probe` with no arguments prints the report, so it can be run on any Linux machine to check detection.

## What is detected
- Maker/model from DMI (Lenovo: product_version; "This computer" when there's no DMI), laptop vs desktop (chassis type + battery)
- Every GPU (PCI class 0x03xx), generation from device-ID ranges, boot GPU, which GPU drives the built-in panel; names tidied from pci.ids ("GA106M [GeForce RTX 3060 Mobile / Max-Q]" → "GeForce RTX 3060 Mobile")
- GPU MUX: ASUS `gpu_mux_mode` (asus-armoury since 6.19, asus-wmi before; 0 = dGPU, 1 = Optimus); other brands inferred from the panel
- Secure Boot, CPU vendor, Intel audio DSP (SOF), Broadcom wl-only Wi-Fi, fingerprint readers (libfprint vendors)

## NVIDIA
| Generation | Arch | Debian 13 | Fedora | openSUSE TW |
|---|---|---|---|---|
| Turing+ (GTX 16, RTX 20–50) | nvidia-open (+lib32) | nvidia-driver + nvidia-open-kernel-dkms (550) | akmod-nvidia (RPM Fusion nonfree) | G07 signed open kmp (OSS) + G07 userspace (NVIDIA repo) |
| Blackwell (RTX 50) | as above | nouveau + warning (550 too old) | as above | as above |
| Maxwell/Pascal/Volta | nvidia-580xx-dkms (AUR) | nvidia-kernel-dkms (550) | akmod-nvidia-580xx | G06 (NVIDIA repo) |
| Kepler and older | nouveau | nouveau | nouveau | nouveau |

Hybrid laptops: switcheroo-control (+ nvidia-prime on Arch), nvidia suspend/resume/hibernate services, nvidia-powerd on Ampere+. MUX: both modes set up, switching needs a restart.

## Sources the drivers need
Switched on and locked: RPM Fusion nonfree (Fedora), AUR (Arch legacy), Debian non-free + non-free-firmware (new repo entry), openSUSE's NVIDIA repo (new, on-demand only, configured via openSUSE's own openSUSE-repos-NVIDIA). The NVIDIA repo is an exception to "no vendor repos": it's the only way openSUSE ships NVIDIA userspace.

## Secure Boot
Self-signed modules (dkms/akmods, openSUSE G06) → one-time MOK enrollment on first restart, password = user passphrase; shown on the Done page. openSUSE G07 open modules are signed by openSUSE. Arch + Secure Boot → warning (sbctl needs Setup Mode).

## To verify before release
nvidia-open-kernel-dkms (Debian name), firmware-intel-graphics (Debian 13), libva-intel-media-driver (Fedora), G06 kmp meta name (openSUSE), power-profiles-daemon vs tuned on openSUSE. Prefer NVIDIA's supported-gpus.json over ID ranges.
