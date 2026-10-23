# Vision: an independent installer

Borea is an installer in its own right. It is not one distribution's installer, and it is not tied to any distribution's ISO. You start Borea once. Windpad, the same interface for every target, guides you. Each target system is built from its own upstream sources.

This document records that decision. Where other documents conflict with it, this one takes precedence.

## Three independent layers

```text
 Installation environment        Windpad                     Distribution adapters
 ───────────────────────         ───────                     ─────────────────────
 Borea Live (reference)    →     hardware, Wi-Fi,       →    fetch upstream packages
 or any compatible live          distribution choice,        or images, install the
 Linux session                   storage, review             target natively
        │                                                            │
   judged by capabilities                                   bring their own tools;
   never by distro identity                                 never borrow the host's
```

1. **Installation environment (the host).** Borea runs from **Borea Live**, its own neutral live environment, or from a compatible existing live session. The host is judged only by what it can do: UEFI variable access, mount namespaces, loop devices, kernel filesystems, the system bus, NetworkManager, polkit, and a graphical session. Its distribution name appears in diagnostics and never limits which targets you can choose. An Arch ISO is an early test host. It is not a requirement.
2. **Windpad.** One landing pad for every target, with hardware discovery, integrated Wi-Fi, family/distribution/edition selection, storage, and installation review. Windpad behaves the same whichever distribution you choose and whatever environment it runs in.
3. **Distribution adapters.** Each adapter obtains its target from upstream and installs it while preserving the target's native configuration and update model. An adapter brings the target's own tooling with it, such as a signed bootstrap archive, a container image of the target, or an OS image. It never relies on whatever happens to be in the host's `PATH`.

## You never boot the target's ISO

Installing Fedora does not require booting Fedora media, and installing PikaOS does not require booting PikaOS media. Some adapters may still download upstream installation images as **sources**. They verify the image, read its root filesystem, and never boot it. Independence from booting an ISO does not necessarily mean avoiding image files.

Acquisition strategies, which are M0 research candidates and are recorded in `crates/borea-distro`:

| Strategy | Meaning | Candidates |
| --- | --- | --- |
| Package bootstrap | Target's own package manager, run from an upstream bootstrap root or container image | Arch, Fedora, Debian, Tumbleweed |
| Layered | Parent bootstrap, then the derivative's upstream layer; the derivative still resolves its own channel | CachyOS, Omarchy |
| Image deployment | Upstream OS image deployed natively, keeping its update and rollback model | Bazzite |
| Upstream image as source | Root filesystem read from verified upstream media, never booted | Nobara, PikaOS |

Each strategy can be revised when M0 evidence warrants it. The principle stays fixed: targets come from upstream, and tools come from the target's side.

## Host and target are independent from the start

The first vertical slice still installs an Arch target, and an Arch live session is a convenient first host. The architecture does not encode that pairing:

- No code path decides eligibility from the host's `os-release`. A test (`host_distribution_never_restricts_targets`) enforces this.
- Host support and target support are separate matrices. Certification covers *capability profile × target*, not *host distro × target*.
- From M0 onward, every target slice runs from Borea Live **and** from at least one live session of a different family.

## Borea Live

Borea Live is standalone: it is not a remastered distribution ISO. Buildroot compiles it from upstream sources (`live/`). Borea writes its own early boot, and Limine boots it. No distribution's packages, installer media, or ISO tooling are involved.

Borea Live is the reference environment and provides the most predictable experience: known drivers, firmware, NetworkManager, polkit, a compositor for Windpad, and the `borea` bootstrapper. It is built early (see the [roadmap](roadmap.md)), not deferred to a final milestone. Running from other live sessions remains supported where their capabilities suffice. Windpad's Wi-Fi and graphics then depend on what that session ships, and `borea doctor` says so plainly.

Installing from an already-installed system to a second disk is a later capability. Installing over the running root filesystem is never offered.

## What exists now

`borea doctor` (from `crates/borea-bootstrap`) implements this model read-only. It classifies the environment (Borea Live, live session, or installed system), lists host capabilities, scans hardware, and shows how every catalog target would be acquired from this host, along with anything that blocks it. It changes nothing on the machine.
