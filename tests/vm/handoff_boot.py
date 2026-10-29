"""Hand off from Borea Live to Omarchy's own installer, in a disposable VM.

Builds a Borea stick image (the composed ISO plus the BOREA_DATA partition that
borea-rataTUI adds, already holding the pinned Omarchy ISO), boots it in UEFI
QEMU with a disposable target disk, runs `borea-cli handoff omarchy` from a
serial root shell, and requires Omarchy's live installer system to come up after
the kexec. For `--layout alongside` the target holds a synthetic Windows layout
(ESP, MSR, NTFS with a sentinel file); afterwards the NTFS partition must have
shrunk by exactly the requested size and still be consistent, with the sentinel
intact. With `--plain-stick` the stick holds only the image, as dd or Etcher
leave it: Borea Live must add the data partition itself and download Omarchy
there. With `--install` the VM goes all the way: a cidata drive (Omarchy's own
unattended mode, test only) installs Omarchy onto the target, the VM restarts
into it, and the extras given with `--extras` must be installed on its first
start by Borea's first-start service, which then removes itself. Only regular
files are ever attached; never a host block device.
"""
import argparse
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import tempfile
import time

from compose_boot import instrument
from console import Console

GIB = 1 << 30
MIB = 1 << 20
ISO = "omarchy-4.0.6.iso"
LINUX_SIZE = 40 * GIB
TARGET = 100 * GIB
BOOT = ("console=tty0 console=ttyS0,115200 loglevel=4 systemd.show_status=yes "
        "borea.handoff-console=ttyS0,115200")
# Test only: the installed system powers off once Borea's extras are in.
POWEROFF = " borea.handoff-poweroff"


def sh(*args, **kw):
    print("+", " ".join(map(str, args)), flush=True)
    return subprocess.run(list(map(str, args)), check=True, **kw)


def stick(borea_iso, omarchy, work, plain, boot=BOOT):
    """The Borea stick: image, then partition 4 (ext4 BOREA_DATA) holding Omarchy.
    A plain stick has only the image, then leftovers from the stick's past."""
    image = work / "stick.img"
    instrument(borea_iso, image, boot)
    if plain:
        start = (image.stat().st_size + MIB - 1) // MIB * MIB
        os.truncate(image, start + (omarchy / ISO).stat().st_size + 3 * GIB)
        with open(image, "r+b") as f:
            f.seek(start)
            f.write(os.urandom(8 * MIB))
        return image
    staged = work / "data"
    (staged / "borea").mkdir(parents=True)
    payload = 0
    for name in [ISO, ISO + ".sha256", ISO + ".sig"]:
        source = omarchy / name
        try:
            os.link(source, staged / "borea" / name)
        except OSError:
            shutil.copyfile(source, staged / "borea" / name)
        payload += source.stat().st_size
    start = (image.stat().st_size + MIB - 1) // MIB * MIB
    size = (payload + 2 * GIB) // MIB * MIB
    os.truncate(image, start + size)
    # The same rule as borea-flash::datapart: entry 4, from the first MiB after the image.
    sh("sfdisk", "--append", "--no-reread", "--wipe", "never", image,
       input=f"start={start // 512}, type=83\n".encode())
    sh("mkfs.ext4", "-q", "-F", "-L", "BOREA_DATA", "-d", staged,
       "-E", f"offset={start}", image, f"{size // 1024}k")
    shutil.rmtree(staged)
    return image


def loop(image):
    return subprocess.run(["sudo", "losetup", "--find", "--show", "--partscan", str(image)],
                          check=True, capture_output=True, text=True).stdout.strip()


def target(work, layout):
    disk = work / "target.img"
    with open(disk, "wb") as f:
        f.truncate(TARGET)
    if layout == "whole":
        return disk, None
    sh("sfdisk", "--no-reread", disk, input=(
        "label: gpt\n"
        "size=100MiB, type=C12A7328-F81F-11D2-BA4B-00A0C93EC93B, name=\"EFI system partition\"\n"
        "size=16MiB, type=E3C9E316-0B5C-4DB8-817D-F92DF00215AE, name=\"Microsoft reserved partition\"\n"
        "type=EBD0A0A2-B9E5-4433-87C0-68B6B72699C7, name=\"Basic data partition\"\n").encode())
    dev = loop(disk)
    try:
        sh("sudo", "udevadm", "settle")
        sh("sudo", "mkfs.fat", "-F", "32", "-n", "SYSTEM", f"{dev}p1")
        sh("sudo", "mkntfs", "--quick", "--label", "Windows", f"{dev}p3")
        mnt = Path(tempfile.mkdtemp(prefix="ntfs-", dir=work))
        sh("sudo", "mount", "-t", "ntfs-3g", f"{dev}p3", mnt)
        sh("sudo", "sh", "-c", f"echo borea-windows-sentinel > {mnt}/sentinel.txt")
        sh("sudo", "umount", mnt)
    finally:
        sh("sudo", "losetup", "--detach", dev)
    return disk, ntfs_size(disk)


def ntfs_size(disk):
    table = json.loads(subprocess.run(["sfdisk", "--json", str(disk)], check=True,
                                      capture_output=True, text=True).stdout)
    return table["partitiontable"]["partitions"][2]["size"] * 512


def check_windows(disk, before):
    after = ntfs_size(disk)
    want = (before - LINUX_SIZE) // MIB * MIB
    if after != want:
        raise AssertionError(f"Windows is {after} bytes, expected {want} (shrunk by {LINUX_SIZE})")
    dev = loop(disk)
    try:
        sh("sudo", "udevadm", "settle")
        sh("sudo", "ntfsfix", "--no-action", f"{dev}p3")
        # ntfsresize marks the volume for a Windows check (chkdsk) on its next boot,
        # as it should; ntfscat reads such a volume, read-only, only when forced.
        out = subprocess.run(["sudo", "ntfscat", "--force", f"{dev}p3", "sentinel.txt"], check=True,
                             capture_output=True, text=True).stdout
        if "borea-windows-sentinel" not in out:
            raise AssertionError("the Windows sentinel file is gone")
        if subprocess.run(["sudo", "blkid", "-s", "TYPE", "-o", "value", f"{dev}p1"],
                          capture_output=True, text=True).stdout.strip() != "vfat":
            raise AssertionError("the Windows ESP is gone")
    finally:
        sh("sudo", "losetup", "--detach", dev)
    print(f"PASS: Windows shrunk by exactly {LINUX_SIZE // GIB} GiB, NTFS consistent, data intact")


def cidata(work):
    """Omarchy's unattended install (its integration tests' format, at the
    4.0.6-era omarchy-iso): the configurator's output files on a drive labelled
    cidata, for a full-disk install onto /dev/vda."""
    files = work / "cidata"
    files.mkdir()
    password = subprocess.run(["openssl", "passwd", "-6", "omarchy"], check=True,
                              capture_output=True, text=True).stdout.strip()
    (files / "user_credentials.json").write_text(json.dumps({
        "root_enc_password": password,
        "users": [{"enc_password": password, "groups": [], "sudo": True,
                   "username": "omarchy"}]}, indent=2))
    boot_start, boot_size = MIB, 2 * GIB
    main_start = boot_start + boot_size
    main_size = TARGET - main_start - MIB
    def part(**kw):
        size = lambda v: {"sector_size": {"unit": "B", "value": 512}, "unit": "B", "value": v}
        kw["size"], kw["start"] = size(kw["size"]), size(kw["start"])
        return {"dev_path": None, "status": "create", "type": "primary", **kw}
    config = {
        "app_config": None, "archinstall-language": "English", "auth_config": {},
        "audio_config": {"audio": "pipewire"},
        "bootloader_config": {"bootloader": "Limine", "uki": False, "removable": False},
        "custom_commands": [],
        "omarchy_install": {
            "mode": "full_disk", "defer_provisioning": False, "target_mount": "/mnt",
            "boot": {"esp_mount": "/boot", "esp_path": "/EFI/limine",
                     "efi_binary": "limine_x64.efi", "enable_fallback": True},
            "storage": {"kernel": "linux-omarchy"}},
        "disk_config": {"config_type": "default_layout", "device_modifications": [{
            "device": "/dev/vda", "wipe": True, "partitions": [
                part(btrfs=[], flags=["boot", "esp"], fs_type="fat32", mount_options=[],
                     mountpoint="/boot", obj_id="ea21d3f2-82bb-49cc-ab5d-6f81ae94e18d",
                     size=boot_size, start=boot_start),
                part(btrfs=[{"mountpoint": "/", "name": "@"},
                            {"mountpoint": "/home", "name": "@home"},
                            {"mountpoint": "/var/log", "name": "@log"},
                            {"mountpoint": "/var/cache/pacman/pkg", "name": "@pkg"}],
                     flags=[], fs_type="btrfs", mount_options=["compress=zstd"],
                     mountpoint=None, obj_id="8c2c2b92-1070-455d-b76a-56263bab24aa",
                     size=main_size, start=main_start)]}]},
        "hostname": "borea-omarchy", "kernels": ["linux-omarchy"],
        "network_config": {"type": "iso"}, "ntp": True, "parallel_downloads": 8,
        "script": None, "services": [], "swap": True, "timezone": "UTC",
        "locale_config": {"kb_layout": "us", "sys_enc": "UTF-8", "sys_lang": "en_US.UTF-8"},
        "mirror_config": {"custom_repositories": [], "mirror_regions": {},
                          "optional_repositories": [], "custom_servers": [
                              {"url": "https://mirror.omarchy.org/$repo/os/$arch"},
                              {"url": "https://geo.mirror.pkgbuild.com/$repo/os/$arch"}]},
        "packages": ["base-devel", "git", "omarchy-keyring", "omarchy-settings", "omarchy"],
        "profile_config": {"gfx_driver": None, "greeter": None, "profile": {}},
        "version": "3.0.9",
    }
    (files / "user_configuration.json").write_text(json.dumps(config, indent=2))
    (files / "user_full_name.txt").write_text("Borea Test\n")
    (files / "user_email_address.txt").write_text("test@example.org\n")
    (files / "user_encrypt_installation.txt").write_text("false\n")
    image = work / "cidata.img"
    os.truncate(image, 16 * MIB)
    sh("mkfs.ext4", "-q", "-F", "-L", "cidata", "-d", files, image)
    return image


def check_extras(disk, work, packages):
    """The installed system has the extras, and Borea's first-start service is gone."""
    dev = loop(disk)
    root = Path(tempfile.mkdtemp(prefix="installed-", dir=work))
    logs = Path(tempfile.mkdtemp(prefix="installed-log-", dir=work))
    try:
        sh("sudo", "udevadm", "settle")
        sh("sudo", "mount", "-o", "ro,subvol=@", f"{dev}p2", root)
        sh("sudo", "mount", "-o", "ro,subvol=@log", f"{dev}p2", logs)
        journal = subprocess.run(
            ["sudo", "journalctl", f"--directory={logs}/journal", "--no-pager",
             "-u", "borea-extras.service"], capture_output=True, text=True).stdout
        (work / "installed-borea-extras.log").write_text(journal)
        local = subprocess.run(["sudo", "ls", f"{root}/var/lib/pacman/local"], check=True,
                               capture_output=True, text=True).stdout.split()
        missing = [p for p in packages if not any(re.fullmatch(rf"{re.escape(p)}-[^-]+-[^-]+", d)
                                                  for d in local)]
        if missing:
            raise AssertionError(f"not installed on first start: {missing}; "
                                 f"service log: {journal.strip().splitlines()[-3:]}")
        leftovers = [f for f in ["usr/local/lib/borea/extras",
                                 "etc/systemd/system/borea-extras.service"]
                     if subprocess.run(["sudo", "test", "-e", f"{root}/{f}"]).returncode == 0]
        if leftovers:
            raise AssertionError(f"Borea's first-start service did not remove itself: {leftovers}")
    finally:
        subprocess.run(["sudo", "umount", root, logs])
        sh("sudo", "losetup", "--detach", dev)
    print(f"PASS: Omarchy's first start added {', '.join(packages)} and Borea's service removed itself")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("borea_iso", type=Path)
    parser.add_argument("omarchy", type=Path, help="directory with the pinned ISO, .sha256 and .sig")
    parser.add_argument("--layout", choices=["whole", "alongside"], required=True)
    parser.add_argument("--work", type=Path, default=Path(__file__).resolve().parents[2] / "target/vm")
    parser.add_argument("--plain-stick", action="store_true",
                        help="no data partition: Borea Live adds it and downloads Omarchy itself")
    parser.add_argument("--install", action="store_true",
                        help="install Omarchy unattended (cidata) and boot it once")
    parser.add_argument("--extras", default="", help="comma-separated extras, e.g. printing")
    parser.add_argument("--name", help="work directory name (default: the layout)")
    parser.add_argument("--timeout", type=int, default=900)
    args = parser.parse_args()
    for f in [args.borea_iso, args.omarchy / ISO]:
        if not f.resolve().is_file():
            raise ValueError(f"{f} must be a regular file, never a host block device")
    work = args.work / f"handoff-{args.name or args.layout}"
    shutil.rmtree(work, ignore_errors=True)
    work.mkdir(parents=True)
    image = stick(args.borea_iso.resolve(), args.omarchy.resolve(), work, args.plain_stick,
                  BOOT + (POWEROFF if args.install else ""))
    if args.plain_stick:
        args.timeout = max(args.timeout, 2700)
    disk, windows = target(work, args.layout)
    firmware = Path("/usr/share/OVMF")
    shutil.copyfile(firmware / "OVMF_VARS_4M.fd", work / "vars.fd")
    command = [
        "qemu-system-x86_64", "-accel",
        "kvm" if os.access("/dev/kvm", os.R_OK | os.W_OK) else "tcg",
        "-m", "6144", "-smp", "2", "-vga", "virtio", "-display", "none",
        "-drive", f"if=pflash,format=raw,readonly=on,file={firmware / 'OVMF_CODE_4M.fd'}",
        "-drive", f"if=pflash,format=raw,file={work / 'vars.fd'}",
        "-device", "qemu-xhci,id=usb",
        # Writable: Borea Live keeps hand-off downloads on the stick's data partition.
        "-drive", f"if=none,id=live,format=raw,file={image}",
        "-device", "usb-storage,drive=live,removable=on,bootindex=1",
        "-drive", f"if=none,id=target,format=raw,file={disk}",
        # Once installed, the target boots first; until then the firmware skips it.
        "-device", "virtio-blk-pci,drive=target,serial=BOREA-TEST-TARGET"
        + (",bootindex=0" if args.install else ""),
        "-nic", "user,model=virtio-net-pci",
        "-serial", f"unix:{work / 'serial.sock'},server=on,wait=off",
    ]
    if args.install:
        # After the target, so it is vdb and the target stays vda.
        command += ["-drive", f"if=none,id=cidata,format=raw,file={cidata(work)}",
                    "-device", "virtio-blk-pci,drive=cidata"]
    choice = (["--whole", "/dev/vda"] if args.layout == "whole"
              else ["--alongside", "/dev/vda3", "--size", f"{LINUX_SIZE // GIB}G"])
    if args.extras:
        choice += ["--extras", args.extras]
    handoff = " ".join(["borea-cli", "handoff", "omarchy", *choice])
    console = None
    with (work / "qemu.log").open("w") as log:
        vm = subprocess.Popen(command, stdout=log, stderr=subprocess.STDOUT)
        try:
            console = Console(work / "serial.sock", work / "console.log")
            console.expect(r"login:", 600)
            console.send("root")
            console.expect(r"[#$] ", 60)
            # The GUI runs as the live `borea` user: it must reach the service too.
            if console.run("runuser -u borea -- borea-cli events >/dev/null", 60) != 0:
                raise AssertionError("the live user (the GUI's) cannot reach borea-installd")
            if console.run(f"{handoff} > /tmp/review.json", 300) != 0:
                raise AssertionError("handoff review failed")
            console.run("cat /tmp/review.json", 30)
            status = console.run(
                f"{handoff} --approval \"$(sed -n 's/.*\"approval\": *\"\\([0-9a-f]*\\)\".*/\\1/p' /tmp/review.json)\"",
                args.timeout)
            if status != 0:
                raise AssertionError(f"the hand-off failed (exit {status})")
            print("PASS: Borea verified Omarchy and loaded its installer", flush=True)
            # After the kexec: Omarchy's kernel, then its live system, which logs
            # root in on the serial console by itself.
            console.expect(r"Linux version", 300)
            m = console.expect(
                r"(archiso login:)|"
                r"(ERROR: .*did not show up|Kernel panic|emergency mode|Failed to mount)", args.timeout)
            if not m.group(1):
                raise AssertionError(f"Omarchy's installer did not start: {m.group(0)}")
            # Omarchy logs root in by itself only on its own screen (tty1); on the
            # serial console, log in as root, which has no password on the live ISO.
            # Its prompt is coloured; rather than match it, run a command until the
            # shell answers.
            console.send("root")
            for attempt in range(6):
                try:
                    if console.run("true", 30) == 0:
                        break
                except TimeoutError:
                    if "login:" in console.buf:
                        console.send("root")
            else:
                raise AssertionError("Omarchy's serial shell never answered")
            # Omarchy's installer is there, and its system came from the ISO file on
            # the Borea stick's data partition (archiso's loop boot).
            if console.run("test -x /root/configurator", 60) != 0:
                raise AssertionError("Omarchy's installer (configurator) is missing")
            if console.run("findmnt -n -o SOURCE,FSTYPE /run/archiso/img_dev | grep -q ext4", 60) != 0:
                raise AssertionError("Omarchy did not boot from the ISO file on the Borea stick")
            if args.plain_stick and console.run("blkid -L BOREA_DATA | grep -q '4$'", 60) != 0:
                raise AssertionError("Omarchy's kernel does not see the data partition Borea added")
            if args.layout == "alongside" and console.run(
                    "parted -ms /dev/vda unit GiB print free | grep -q ':free;'", 60) != 0:
                raise AssertionError("Omarchy sees no free space beside Windows")
            print("PASS: Omarchy's installer system started from the ISO on the Borea stick",
                  flush=True)
            if args.install:
                install_and_first_start(console, vm, args)
        finally:
            if console:
                console.close()
            vm.terminate()
            try:
                vm.wait(timeout=20)
            except subprocess.TimeoutExpired:
                vm.kill()
                vm.wait()
    if windows is not None:
        check_windows(disk, windows)
    if args.plain_stick:
        check_stick(image)
    if args.install:
        check_extras(disk, work, [p for e in args.extras.split(",") if e for p in EXTRAS[e]])


# The packages each extra adds on Omarchy (crates/borea-handoff/src/upstream.rs).
EXTRAS = {"printing": ["cups", "system-config-printer"], "flatpak": ["flatpak"]}


def install_and_first_start(console, vm, args):
    """Omarchy's unattended install runs on tty1; then Borea's stage service must
    have put its first-start service in, and the installed system must power off
    by itself once that service has added the extras."""
    failed = "jq -e '[.phases[]? | select(.status==\"failed\")] | length > 0' /run/omarchy-install/state.json"
    done = "jq -e '.finished_at != null' /run/omarchy-install/state.json"
    for _ in range(args.timeout // 20 * 4):
        if console.run(f"{failed} >/dev/null 2>&1", 60) == 0:
            console.run("jq -c '.phases[-1]' /run/omarchy-install/state.json; tail -n 20 /var/log/omarchy-install.log", 60)
            raise AssertionError("Omarchy's unattended install failed")
        if console.run(f"{done} >/dev/null 2>&1", 60) == 0:
            break
        time.sleep(20)
    else:
        raise AssertionError("Omarchy's unattended install did not finish")
    print("PASS: Omarchy installed itself onto the target", flush=True)
    for _ in range(12):
        if console.run("journalctl -b -u borea-extras-stage.service --no-pager | grep -q 'staged for the first start'", 60) == 0:
            break
        time.sleep(10)
    else:
        console.run("systemctl status borea-extras-stage.service --no-pager; journalctl -b -u borea-extras-stage.service --no-pager | tail", 60)
        raise AssertionError("Borea did not stage its extras into the installed system")
    print("PASS: Borea staged its first-start service into the installed system", flush=True)
    console.send("systemctl reboot")
    try:
        vm.wait(timeout=args.timeout * 3)
    except subprocess.TimeoutExpired:
        raise AssertionError("the installed system did not power off after its first start "
                             "(Borea's extras service did not finish)") from None
    print("PASS: the installed system started and powered off by itself", flush=True)


def check_stick(image):
    parts = json.loads(subprocess.run(["sfdisk", "--json", str(image)], check=True,
                                      capture_output=True, text=True).stdout)["partitiontable"]["partitions"]
    data = [p for p in parts if p["node"].endswith("4")]
    if len(parts) != 4 or not data or data[0]["type"] != "83":
        raise AssertionError(f"the stick's partitions are wrong: {parts}")
    end = max(p["start"] + p["size"] for p in parts if p is not data[0])
    if data[0]["start"] * 512 < end * 512 or data[0]["start"] * 512 % MIB:
        raise AssertionError("the data partition overlaps Borea Live or is unaligned")
    print("PASS: Borea Live added its data partition after the image, leaving the image intact")


def summary(work, error):
    """A short failure report the workflow turns into annotations (GitHub shows ten)."""
    console = work / "console.log"
    lines = console.read_text(errors="replace").splitlines() if console.exists() else []
    tail = [l.strip() for l in lines if l.strip() and not l.strip().startswith('"')][-7:]
    (work / "failure.txt").write_text("\n".join([f"{type(error).__name__}: {error}"[:500], *tail]) + "\n")


if __name__ == "__main__":
    import sys
    layout = next((sys.argv[i + 1] for i, a in enumerate(sys.argv) if a == "--name"),
                  next((sys.argv[i + 1] for i, a in enumerate(sys.argv) if a == "--layout"), "unknown"))
    work_arg = next((sys.argv[i + 1] for i, a in enumerate(sys.argv) if a == "--work"), None)
    try:
        main()
    except Exception as error:
        base = Path(work_arg) if work_arg else Path(__file__).resolve().parents[2] / "target/vm"
        summary(base / f"handoff-{layout}", error)
        raise
