"""Hand off from Borea Live to Omarchy's own installer, in a disposable VM.

Builds a Borea stick image (the composed ISO plus the BOREA_DATA partition that
borea-rataTUI adds, already holding the pinned Omarchy ISO), boots it in UEFI
QEMU with a disposable target disk, runs `borea-cli handoff omarchy` from a
serial root shell, and requires Omarchy's live installer system to come up after
the kexec. For `--layout alongside` the target holds a synthetic Windows layout
(ESP, MSR, NTFS with a sentinel file); afterwards the NTFS partition must have
shrunk by exactly the requested size and still be consistent, with the sentinel
intact. Only regular files are ever attached; never a host block device.
"""
import argparse
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import tempfile

from compose_boot import instrument
from console import Console

GIB = 1 << 30
MIB = 1 << 20
ISO = "omarchy-4.0.6.iso"
LINUX_SIZE = 40 * GIB
TARGET = 100 * GIB
BOOT = ("console=tty0 console=ttyS0,115200 loglevel=4 systemd.show_status=yes "
        "borea.handoff-console=ttyS0,115200")


def sh(*args, **kw):
    print("+", " ".join(map(str, args)), flush=True)
    return subprocess.run(list(map(str, args)), check=True, **kw)


def stick(borea_iso, omarchy, work):
    """The Borea stick: image, then partition 4 (ext4 BOREA_DATA) holding Omarchy."""
    image = work / "stick.img"
    instrument(borea_iso, image, BOOT)
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
        out = subprocess.run(["sudo", "ntfscat", f"{dev}p3", "sentinel.txt"], check=True,
                             capture_output=True, text=True).stdout
        if "borea-windows-sentinel" not in out:
            raise AssertionError("the Windows sentinel file is gone")
        if subprocess.run(["sudo", "blkid", "-s", "TYPE", "-o", "value", f"{dev}p1"],
                          capture_output=True, text=True).stdout.strip() != "vfat":
            raise AssertionError("the Windows ESP is gone")
    finally:
        sh("sudo", "losetup", "--detach", dev)
    print(f"PASS: Windows shrunk by exactly {LINUX_SIZE // GIB} GiB, NTFS consistent, data intact")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("borea_iso", type=Path)
    parser.add_argument("omarchy", type=Path, help="directory with the pinned ISO, .sha256 and .sig")
    parser.add_argument("--layout", choices=["whole", "alongside"], required=True)
    parser.add_argument("--work", type=Path, default=Path(__file__).resolve().parents[2] / "target/vm")
    parser.add_argument("--timeout", type=int, default=900)
    args = parser.parse_args()
    for f in [args.borea_iso, args.omarchy / ISO]:
        if not f.resolve().is_file():
            raise ValueError(f"{f} must be a regular file, never a host block device")
    work = args.work / f"handoff-{args.layout}"
    shutil.rmtree(work, ignore_errors=True)
    work.mkdir(parents=True)
    image = stick(args.borea_iso.resolve(), args.omarchy.resolve(), work)
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
        "-device", "virtio-blk-pci,drive=target,serial=BOREA-TEST-TARGET",
        "-nic", "user,model=virtio-net-pci",
        "-serial", f"unix:{work / 'serial.sock'},server=on,wait=off",
    ]
    choice = (["--whole", "/dev/vda"] if args.layout == "whole"
              else ["--alongside", "/dev/vda3", "--size", f"{LINUX_SIZE // GIB}G"])
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
            if args.layout == "alongside" and console.run(
                    "parted -ms /dev/vda unit GiB print free | grep -q ':free;'", 60) != 0:
                raise AssertionError("Omarchy sees no free space beside Windows")
            print("PASS: Omarchy's installer system started from the ISO on the Borea stick",
                  flush=True)
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


def summary(work, error):
    """A short failure report the workflow turns into annotations (GitHub shows ten)."""
    console = work / "console.log"
    lines = console.read_text(errors="replace").splitlines() if console.exists() else []
    tail = [l.strip() for l in lines if l.strip() and not l.strip().startswith('"')][-7:]
    (work / "failure.txt").write_text("\n".join([f"{type(error).__name__}: {error}"[:500], *tail]) + "\n")


if __name__ == "__main__":
    import sys
    layout = next((sys.argv[i + 1] for i, a in enumerate(sys.argv) if a == "--layout"), "unknown")
    work_arg = next((sys.argv[i + 1] for i, a in enumerate(sys.argv) if a == "--work"), None)
    try:
        main()
    except Exception as error:
        base = Path(work_arg) if work_arg else Path(__file__).resolve().parents[2] / "target/vm"
        summary(base / f"handoff-{layout}", error)
        raise
