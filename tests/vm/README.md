# Arch installation VM test

Run as an ordinary user with KVM access. Requires Python 3.11+, QEMU, xorriso,
OVMF and an already-built Borea Live ISO. Internet access is needed in the VM.

```sh
python tests/vm/arch_install.py
# Or pin the input explicitly:
python tests/vm/arch_install.py --iso /absolute/path/to/borea-live.iso
python -m unittest discover -s tests/vm -v
```

The harness creates a new sparse 20 GiB disk under `target/vm/arch-*` for each
run. QEMU receives only files created for that run, including a private copy of
the selected ISO and writable OVMF variables. It never receives a host block
device. Processes are stopped through their own Popen handles.

The chosen ISO supplies both the system image and the extracted kernel/initrd.
The live stage uses direct kernel boot with a serial console and read-only
virtio installation media. The installed disk subsequently boots through OVMF
and its own bootloader, with the installation medium absent.

A pass requires installation, normal-user login, mounted root and ESP, active
NetworkManager, a successful kernel package transaction, reboot, and login
again. A fresh rolling installation may already have the newest kernel: the
transaction then exercises reinstallation hooks, rather than proving migration
between two different kernel versions. That upgrade needs a separate versioned
fixture. Pipeline failures are checked after every login.

`result.json`, `serial.log`, `qemu.log`, the ISO and disk remain in the run
directory for diagnosis. The password is a fixed disposable test value, visible
in the serial log; never use these accounts or disks as a real installation.

Do not run alongside Claude's existing scratch VM test. The lock prevents
concurrent runs of this harness only. Python checks run without launching QEMU.
