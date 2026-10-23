"""Fast harness checks; no VM, network, root, or block device required."""
import re
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

import arch_install
from console import Console


class HarnessTests(unittest.TestCase):
    def test_boot_file_checks_use_sudo_and_preserve_the_complete_command(self):
        import shlex
        command = 'test -s /boot/vmlinuz-linux && test -s /boot/initramfs-linux.img'
        elevated = arch_install.as_root(command)
        arguments = shlex.split(elevated.split(' | ', 1)[1])
        self.assertEqual(arguments, ['sudo', '-S', '--', 'sh', '-c', command])

    def test_newest_image_is_selected_without_a_hardcoded_date(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            output = root / 'target/live/out'
            output.mkdir(parents=True)
            old = output / 'borea-live-old.iso'
            new = output / 'borea-live-new.iso'
            old.write_bytes(b'old')
            new.write_bytes(b'new')
            import os
            os.utime(old, ns=(1, 1))
            os.utime(new, ns=(2, 2))
            with patch.object(arch_install, 'REPO', root):
                self.assertEqual(arch_install.select_iso(), new)

    def test_missing_explicit_image_is_rejected(self):
        with tempfile.TemporaryDirectory() as directory:
            with self.assertRaises(FileNotFoundError):
                arch_install.select_iso(str(Path(directory) / 'missing.iso'))

    def test_command_failure_cannot_pass(self):
        from unittest.mock import Mock
        console = Mock()
        console.run.return_value = 1
        with self.assertRaisesRegex(RuntimeError, 'Guest command failed'):
            arch_install.require(console, 'false | tail')

    def test_every_command_enables_pipefail(self):
        console = Console.__new__(Console)
        sent = []
        console.send = sent.append
        console.expect = lambda pattern, timeout: re.match(pattern, sent[-1].split('echo ')[-1].replace('$?', '1'))
        self.assertEqual(console.run('false | tail'), 1)
        self.assertTrue(sent[0].startswith('set -o pipefail; '))

    def test_serial_eof_fails_immediately(self):
        from unittest.mock import Mock
        console = Console.__new__(Console)
        console.s = Mock()
        console.s.recv.return_value = b''
        with self.assertRaises(ConnectionError):
            console.read()

    def test_send_uses_complete_write(self):
        from unittest.mock import Mock
        console = Console.__new__(Console)
        console.s = Mock()
        console.send('uname -r')
        console.s.sendall.assert_called_once_with(b'uname -r\n')

    def test_shutdown_drains_console_until_vm_exits(self):
        from unittest.mock import Mock
        vm = arch_install.VM.__new__(arch_install.VM)
        vm.process = Mock()
        vm.process.poll.side_effect = [None, None, 0]
        vm.console = Mock()
        vm.poweroff()
        self.assertEqual(vm.console.read.call_count, 2)

    def test_shutdown_accepts_serial_eof_and_waits_for_process(self):
        from unittest.mock import Mock
        vm = arch_install.VM.__new__(arch_install.VM)
        vm.process = Mock()
        vm.process.poll.side_effect = [None, 0]
        vm.console = Mock()
        vm.console.read.side_effect = ConnectionError('serial closed')
        vm.poweroff()
        vm.process.wait.assert_called_once()


if __name__ == '__main__':
    unittest.main()
