"""Disposable Arch installation test. No host block devices are passed to QEMU."""
import argparse
import fcntl
import hashlib
import json
import os
from pathlib import Path
import shlex
import shutil
import subprocess
import tempfile
import time

from console import Console

REPO = Path(__file__).resolve().parents[2]
PASSWORD = 'borea-disposable-vm-only'


def select_iso(explicit=None):
    if explicit:
        path = Path(explicit).resolve()
        if not path.is_file():
            raise FileNotFoundError(path)
        return path
    images = list((REPO / 'target/live/out').glob('borea-live-*.iso'))
    if not images:
        raise FileNotFoundError('Build Borea Live first, or pass --iso PATH')
    return max(images, key=lambda p: p.stat().st_mtime_ns)


def require(console, command, timeout=60):
    status = console.run(command, timeout)
    if status:
        raise RuntimeError(f'Guest command failed ({status}): {command}')


def as_root(command):
    """Read root-only boot files through the test user's configured sudo access."""
    return f'printf "%s\\n" {shlex.quote(PASSWORD)} | sudo -S -- sh -c {shlex.quote(command)}'


def login(console, user, password=None):
    console.expect(r'login: ', 300)
    console.send(user)
    if password:
        console.expect(r'Password: ', 30)
        console.send(password)
    console.expect(r'[#$] ', 60)
    require(console, 'stty cols 200; export TERM=dumb')


class VM:
    def __init__(self, directory, firmware, extra):
        self.log = open(directory / 'qemu.log', 'ab')
        self.process = subprocess.Popen([
            'qemu-system-x86_64', '-enable-kvm', '-m', '4G', '-smp', '4',
            '-vga', 'std', '-display', 'none',
            '-drive', f'if=pflash,format=raw,readonly=on,file={firmware}',
            '-drive', f'if=pflash,format=raw,file={directory / "vars.fd"}',
            '-drive', f'id=target,file={directory / "target.raw"},if=none,format=raw',
            '-device', 'virtio-blk-pci,drive=target,serial=BOREATEST',
            '-nic', 'user,model=virtio-net-pci',
            '-serial', f'unix:{directory / "serial.sock"},server=on,wait=off',
        ] + extra, stdout=self.log, stderr=subprocess.STDOUT)
        try:
            self.console = Console(str(directory / 'serial.sock'), str(directory / 'serial.log'))
        except BaseException:
            self.close()
            raise

    def close(self):
        if hasattr(self, 'console'):
            self.console.close()
        if self.process.poll() is None:
            self.process.terminate()
            try:
                self.process.wait(timeout=10)
            except subprocess.TimeoutExpired:
                self.process.kill()
                self.process.wait()
        self.log.close()

    def poweroff(self):
        deadline = time.monotonic() + 60
        try:
            while self.process.poll() is None:
                remaining = deadline - time.monotonic()
                if remaining <= 0:
                    raise subprocess.TimeoutExpired(self.process.args, 60)
                try:
                    # Keep draining shutdown output: a full serial socket can
                    # block the guest's console and prevent it powering off.
                    self.console.read()
                except ConnectionError:
                    # Serial EOF commonly precedes QEMU exiting. Nothing more
                    # can be drained; wait only for the remaining deadline.
                    self.process.wait(timeout=remaining)
        except subprocess.TimeoutExpired as error:
            raise RuntimeError('Guest did not power off cleanly') from error


def say(text):
    print(time.strftime('[%H:%M:%S] ')+text, flush=True)


def run(args):
    if os.geteuid() == 0:
        raise RuntimeError('Run as an ordinary user with /dev/kvm access')
    for tool in ['qemu-system-x86_64', 'xorriso']:
        if not shutil.which(tool):
            raise RuntimeError(f'Missing host tool: {tool}')
    iso = select_iso(args.iso)
    for path in [args.firmware, args.vars]:
        if not Path(path).is_file():
            raise FileNotFoundError(path)
    output = REPO / 'target/vm'
    output.mkdir(parents=True, exist_ok=True)
    # Keep our own runs mutually exclusive. Claude's existing scratch run is separate.
    with open(output / 'arch-install.lock', 'w') as lock:
        fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        directory = Path(tempfile.mkdtemp(prefix='arch-', dir=output))
        say(f'Artifacts: {directory}')
        # Immutable per-run copy: a concurrent image build cannot replace this medium.
        image = directory / 'live.iso'
        shutil.copyfile(iso, image)
        with image.open('rb') as source:
            digest = hashlib.file_digest(source, 'sha256').hexdigest()
        (directory / 'result.json').write_text(json.dumps({
            'status': 'running', 'source_iso': str(iso), 'iso_sha256': digest,
        }, indent=2))
        # Extract from the chosen ISO; never mix its rootfs with another build's initrd.
        subprocess.run(['xorriso', '-osirrox', 'on', '-indev', str(image),
                        '-extract', '/boot/vmlinuz', str(directory / 'vmlinuz'),
                        '-extract', '/boot/initramfs.img', str(directory / 'initramfs.img')],
                       check=True, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        shutil.copyfile(args.vars, directory / 'vars.fd')
        with open(directory / 'target.raw', 'xb') as disk:
            disk.truncate(20 << 30)
        vm = None
        stage = 'install'
        try:
            say('Booting Borea Live and installing Arch Minimal')
            vm = VM(directory, args.firmware, [
                '-drive', f'file={image},if=virtio,format=raw,readonly=on',
                '-kernel', str(directory / 'vmlinuz'),
                '-initrd', str(directory / 'initramfs.img'),
                '-append', 'loglevel=3 console=tty0 console=ttyS0,115200',
            ])
            c = vm.console
            login(c, 'root')
            require(c, 'nm-online -q -t 120', 150)
            require(c, '(for i in $(seq 30); do resolvectl query geo.mirror.pkgbuild.com && exit 0; sleep 2; done; exit 1)', 90)
            require(c, 'borea plan --disk vda --user tester --hostname borea-test '
                    '--timezone Europe/London --locale en_GB.UTF-8 --keymap uk '
                    '--kernel-arg console=ttyS0,115200 -o /root/plan.json '
                    '2>&1 | tee /root/plan.out', 900)
            require(c, "grep -o 'sudo borea-worker.*' /root/plan.out | sed 's/^sudo //' > /root/install.sh; test -s /root/install.sh")
            require(c, f'printf "%s\\n" {shlex.quote(PASSWORD)} | sh /root/install.sh 2>&1 | tee /root/install.out', 3600)
            c.send('poweroff')
            vm.poweroff()
            vm.close()
            vm = None
            (directory / 'serial.sock').unlink(missing_ok=True)
            stage = 'first-boot'
            say('Booting installed disk without installer medium')
            vm = VM(directory, args.firmware, [])
            c = vm.console
            login(c, 'tester', PASSWORD)
            require(c, 'test "$(id -un)" = tester && test -f /etc/arch-release')
            require(c, 'findmnt / && findmnt /boot && systemctl is-active NetworkManager')
            require(c, 'uname -r && pacman -Q linux')
            require(c, as_root('cat /boot/limine.conf'))
            stage = 'kernel-transaction'
            say('Updating packages and reinstalling kernel to exercise boot hooks')
            # pipefail is set for every command, including after the normal-user login.
            require(c, f'printf "%s\\n" {shlex.quote(PASSWORD)} | sudo -S pacman -Syu --noconfirm linux 2>&1 | tail -30', 1800)
            require(c, 'pacman -Q linux')
            require(c, as_root('test -s /boot/vmlinuz-linux && test -s /boot/initramfs-linux.img'))
            stage = 'second-boot'
            c.send(f'printf "%s\\n" {shlex.quote(PASSWORD)} | sudo -S reboot')
            login(c, 'tester', PASSWORD)
            require(c, 'test "$(id -un)" = tester && uname -r && pacman -Q linux && systemctl is-active NetworkManager')
            c.send(f'printf "%s\\n" {shlex.quote(PASSWORD)} | sudo -S poweroff')
            vm.poweroff()
            (directory / 'result.json').write_text(json.dumps({
                'status': 'pass', 'source_iso': str(iso), 'iso_sha256': digest,
                'kernel_check': 'package transaction and reboot; version may be unchanged',
            }, indent=2))
            say('PASS: install, login, kernel transaction and second boot')
        except BaseException as error:
            (directory / 'result.json').write_text(json.dumps({
                'status': 'fail', 'stage': stage, 'error': str(error),
                'source_iso': str(iso), 'iso_sha256': digest,
            }, indent=2))
            raise
        finally:
            if vm:
                vm.close()


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--iso', help='default: newest built Borea Live ISO')
    parser.add_argument('--firmware', default='/usr/share/edk2/x64/OVMF_CODE.4m.fd')
    parser.add_argument('--vars', default='/usr/share/edk2/x64/OVMF_VARS.4m.fd')
    run(parser.parse_args())
