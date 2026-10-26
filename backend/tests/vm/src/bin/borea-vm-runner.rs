#![forbid(unsafe_code)]
//! Run inside the disposable test-tools container; only /work files are ever attached as drives.
use nix::sys::memfd::{MFdFlags, memfd_create};
use std::{
    fs::{self, File},
    io::{Read, Seek, SeekFrom, Write},
    os::fd::AsRawFd,
    path::Path,
    process::{Command, Stdio},
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};
use zeroize::Zeroizing;
fn run() -> Result<(), String> {
    if !Path::new("/work/.borea-test-workdir").is_file() {
        return Err("Missing disposable work-directory marker.".into());
    }
    let boot = std::env::args().nth(1).as_deref() == Some("boot");
    let distro = std::env::var("BOREA_TEST_DISTRO").unwrap_or_else(|_| "debian".into());
    if !["arch", "debian", "fedora", "opensuse"].contains(&distro.as_str()) {
        return Err("Unknown VM test distro.".into());
    }
    let proxy = Zeroizing::new(
        std::env::var("HTTPS_PROXY").or_else(|_| std::env::var("HTTP_PROXY")).unwrap_or_default(),
    );
    let fd = memfd_create("borea-proxy", MFdFlags::empty()).map_err(|e| e.to_string())?;
    let mut memory = File::from(fd);
    memory.write_all(proxy.as_bytes()).map_err(|e| e.to_string())?;
    memory.seek(SeekFrom::Start(0)).map_err(|e| e.to_string())?;
    if !Path::new("/work/target.qcow2").exists() {
        let status = Command::new("/usr/bin/qemu-img")
            .args(["create", "-f", "qcow2", "/work/target.qcow2", "40G"])
            .status()
            .map_err(|e| e.to_string())?;
        if !status.success() {
            return Err("Cannot create the disposable virtual disk.".into());
        }
    }
    if !Path::new("/work/OVMF_VARS.fd").exists() {
        fs::copy("/usr/share/OVMF/OVMF_VARS_4M.fd", "/work/OVMF_VARS.fd")
            .map_err(|e| e.to_string())?;
    }
    let mut command = Command::new("/usr/bin/qemu-system-x86_64");
    command.args([
        "-machine",
        "q35",
        "-accel",
        "tcg",
        "-cpu",
        "max",
        "-smp",
        "2",
        "-m",
        "4096",
        "-display",
        "none",
        "-serial",
        "stdio",
        "-monitor",
        "none",
        "-no-reboot",
        "-drive",
        "if=pflash,format=raw,readonly=on,file=/usr/share/OVMF/OVMF_CODE_4M.fd",
        "-drive",
        "if=pflash,format=raw,file=/work/OVMF_VARS.fd",
        "-drive",
        "if=none,id=target,format=qcow2,file=/work/target.qcow2",
        "-device",
        "virtio-blk-pci,drive=target,serial=BOREA-TEST-TARGET",
        "-netdev",
        "user,id=net0",
        "-device",
        "virtio-net-pci,netdev=net0",
        "-device",
        "virtio-rng-pci",
    ]);
    if !boot {
        command.args([
            "-kernel",
            "/work/vmlinuz",
            "-initrd",
            "/work/live.cpio.gz",
            "-append",
            "console=ttyS0,115200 rdinit=/init panic=-1",
            "-fw_cfg",
            "name=opt/borea/autotest,string=BOREA_DISPOSABLE_VM",
            "-fw_cfg",
            &format!("name=opt/borea/distro,string={distro}"),
            "-fw_cfg",
            "name=opt/borea/ca,file=/run/borea-ca.pem",
            "-fw_cfg",
            &format!("name=opt/borea/proxy,file=/proc/self/fd/{}", memory.as_raw_fd()),
        ]);
    }
    let mut child = command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .map_err(|e| e.to_string())?;
    let mut stdin = child.stdin.take().ok_or("Missing VM console input")?;
    let mut stdout = child.stdout.take().ok_or("Missing VM console output")?;
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let mut buf = [0u8; 4096];
        loop {
            match stdout.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    if tx.send(String::from_utf8_lossy(&buf[..n]).into_owned()).is_err() {
                        break;
                    }
                }
            }
        }
    });
    let mut log = File::create(if boot { "/work/boot.log" } else { "/work/install.log" })
        .map_err(|e| e.to_string())?;
    let start = Instant::now();
    let mut buffer = String::new();
    let mut stage = 0;
    let mut complete = false;
    let mut luks = false;
    let mut net = false;
    let mut sudo = false;
    loop {
        if start.elapsed() > Duration::from_secs(2400) {
            let _ = child.kill();
            let _ = child.wait();
            return Err("The VM exceeded the test time limit.".into());
        }
        match rx.recv_timeout(Duration::from_secs(1)) {
            Ok(text) => {
                let safe = text.replace("borea-disposable-vm-ONLY", "[TEST CREDENTIAL REDACTED]");
                log.write_all(safe.as_bytes()).map_err(|e| e.to_string())?;
                log.flush().map_err(|e| e.to_string())?;
                print!("{safe}");
                let _ = std::io::stdout().flush();
                buffer.push_str(&text);
                if buffer.len() > 32768 {
                    let keep = buffer.len() - 16384;
                    let keep =
                        (keep..buffer.len()).find(|i| buffer.is_char_boundary(*i)).unwrap_or(0);
                    buffer.drain(..keep);
                }
                if !boot {
                    complete |= buffer.contains("BOREA_INSTALL_COMPLETE");
                    continue;
                }
                if buffer.contains("Kernel panic") || buffer.contains("BOREA_TEST_ERROR") {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err("The guest reported a fatal error; inspect its console log.".into());
                }
                luks |= buffer.contains("BOREA_LUKS_OK");
                net |= buffer.contains("BOREA_NET_OK");
                let input = match stage {
                    0 if buffer.contains("Please unlock disk")
                        || buffer.contains("Enter passphrase") =>
                    {
                        stage = 1;
                        Some("borea-disposable-vm-ONLY\n")
                    }
                    1 if buffer.contains("login:") => {
                        stage = 2;
                        Some("boreatest\n")
                    }
                    2 if buffer.contains("Password:") => {
                        stage = 3;
                        Some("borea-disposable-vm-ONLY\n")
                    }
                    3 if buffer.contains("boreatest@borea-test-vm") => {
                        stage = 4;
                        Some("stty -echo\n")
                    }
                    4 if buffer.contains("boreatest@borea-test-vm") => {
                        stage = 5;
                        Some(
                            ". /etc/os-release; test \"$ID\" = debian && echo BOREA_DISTRO_OK; test \"$(id -un)\" = boreatest && echo BOREA_USER_OK; test -b /dev/mapper/borea-root && echo BOREA_LUKS_OK; nm-online -q --timeout=60 && echo BOREA_NET_OK; sudo -S -p '[BOREA_SUDO]' id -u\n",
                        )
                    }
                    5 if buffer.contains("[BOREA_SUDO]") => {
                        stage = 6;
                        Some("borea-disposable-vm-ONLY\n")
                    }
                    6 if buffer.contains("\n0\r\n") || buffer.contains("\n0\n") => {
                        sudo = true;
                        stage = 7;
                        Some("echo BOREA_VM_BOOT_OK; sudo /usr/bin/systemctl poweroff\n")
                    }
                    _ => None,
                };
                if let Some(input) = input {
                    stdin.write_all(input.as_bytes()).map_err(|e| e.to_string())?;
                    stdin.flush().map_err(|e| e.to_string())?;
                    buffer.clear();
                }
                complete |= buffer.contains("BOREA_VM_BOOT_OK");
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
    let status = child.wait().map_err(|e| e.to_string())?;
    if !status.success() || !complete || boot && !(luks && net && sudo) {
        return Err(format!(
            "VM assertion failed: exit={status}, complete={complete}, luks={luks}, network={net}, sudo={sudo}"
        ));
    }
    println!("BOREA_VM_TEST_PASSED {}", if boot { "boot" } else { "install" });
    Ok(())
}
fn main() -> std::process::ExitCode {
    match run() {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{e}");
            std::process::ExitCode::FAILURE
        }
    }
}
