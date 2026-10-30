"""Boot a composed ISO as a read-only USB device and require native GUI frames."""
import argparse
import mmap
import os
from pathlib import Path
import shutil
import subprocess
import tempfile

from console import Console

REPO = Path(__file__).resolve().parents[2]


SMOKE = "console=tty0 console=ttyS0,115200 loglevel=4 systemd.show_status=yes borea.boot-smoke"


def instrument(source, target, cmdline=SMOKE):
    """Only a disposable copy gets serial output and the test's boot flags."""
    original = (REPO / "live/buildroot/board/borea/limine.conf").read_bytes()
    config = f"""timeout: 0
/Borea USB boot test
    protocol: linux
    path: boot():/boot/vmlinuz
    module_path: boot():/boot/initramfs.img
    cmdline: {cmdline}
""".encode()
    if len(config) > len(original):
        raise ValueError("Smoke config exceeds the ISO's config record")
    shutil.copyfile(source, target)
    with target.open("r+b") as file, mmap.mmap(file.fileno(), 0) as image:
        at = image.find(original)
        if at < 0 or image.find(original, at + 1) >= 0:
            raise ValueError("Expected exactly one unmodified Limine config in the unsigned ISO")
        image[at:at + len(original)] = config.ljust(len(original), b"\n")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("iso", type=Path)
    parser.add_argument("--timeout", type=int, default=300)
    args = parser.parse_args()
    iso = args.iso.resolve()
    if not iso.is_file():
        raise ValueError("ISO must be a regular file, never a host block device")
    workspace = REPO / "target/vm/compose-smoke"
    workspace.mkdir(parents=True, exist_ok=True)
    firmware = Path("/usr/share/OVMF")
    with tempfile.TemporaryDirectory(prefix="boot-", dir=workspace) as temporary:
        directory = Path(temporary)
        instrument(iso, directory / "boot.iso")
        shutil.copyfile(firmware / "OVMF_VARS_4M.fd", directory / "vars.fd")
        command = [
            "qemu-system-x86_64", "-accel",
            "kvm" if os.access("/dev/kvm", os.R_OK | os.W_OK) else "tcg",
            "-m", "2048", "-smp", "2", "-vga", "virtio", "-display", "none",
            "-drive", f"if=pflash,format=raw,readonly=on,file={firmware / 'OVMF_CODE_4M.fd'}",
            "-drive", f"if=pflash,format=raw,file={directory / 'vars.fd'}",
            "-device", "qemu-xhci,id=usb",
            "-drive", f"if=none,id=live,format=raw,readonly=on,file={directory / 'boot.iso'}",
            "-device", "usb-storage,drive=live,removable=on,bootindex=1",
            "-nic", "user,model=virtio-net-pci",
            "-serial", f"unix:{directory / 'serial.sock'},server=on,wait=off",
        ]
        console = None
        with (workspace / "qemu.log").open("w") as log:
            vm = subprocess.Popen(command, stdout=log, stderr=subprocess.STDOUT)
            try:
                console = Console(directory / "serial.sock", workspace / "boot.log")
                console.expect("BOREA_GUI_READY", args.timeout)
                print("PASS: read-only UEFI USB boot reached native installer frames")
            finally:
                if console:
                    console.close()
                vm.terminate()
                try:
                    vm.wait(timeout=10)
                except subprocess.TimeoutExpired:
                    vm.kill()
                    vm.wait()


if __name__ == "__main__":
    main()
