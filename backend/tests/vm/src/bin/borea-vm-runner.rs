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
#[path = "../media.rs"]
mod media;
fn run() -> Result<(), String> {
    if fs::read_to_string("/work/.borea-test-workdir").ok().as_deref().map(str::trim)
        != Some("BOREA_TEST_WORK")
    {
        return Err("Missing disposable work-directory marker.".into());
    }
    let boot = std::env::args().nth(1).as_deref() == Some("boot");
    let distro = std::env::var("BOREA_TEST_DISTRO").unwrap_or_else(|_| "debian".into());
    if !["arch", "debian", "fedora", "opensuse"].contains(&distro.as_str()) {
        return Err("Unknown VM test distro.".into());
    }
    let layout = std::env::var("BOREA_TEST_LAYOUT").unwrap_or_else(|_| "whole".into());
    if !["whole", "alongside"].contains(&layout.as_str()) {
        return Err("Unknown VM test layout.".into());
    }
    let secure_boot = match std::env::var("BOREA_TEST_SECURE_BOOT").as_deref() {
        Ok("1") => true,
        Ok("0") | Err(_) => false,
        _ => return Err("BOREA_TEST_SECURE_BOOT must be 0 or 1.".into()),
    };
    if secure_boot && distro == "arch" {
        return Err("Arch uses owner-key enrollment from Setup Mode; the Microsoft-enrolled fixture cannot certify that separate pathway.".into());
    }
    let scenario = format!("{distro} {layout} secure_boot={}\n", u8::from(secure_boot));
    let scenario_path = Path::new("/work/scenario");
    if scenario_path.exists() {
        if fs::read_to_string(scenario_path).map_err(|e| e.to_string())? != scenario {
            return Err("Use a separate disposable work directory for each distro/layout/firmware scenario.".into());
        }
    } else {
        fs::write(scenario_path, scenario).map_err(|e| e.to_string())?;
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
            .args([
                "create",
                "-f",
                "qcow2",
                "/work/target.qcow2",
                if layout == "alongside" { "100G" } else { "40G" },
            ])
            .status()
            .map_err(|e| e.to_string())?;
        if !status.success() {
            return Err("Cannot create the disposable virtual disk.".into());
        }
    }
    if !Path::new("/work/OVMF_VARS.fd").exists() {
        fs::copy(
            if secure_boot {
                "/usr/share/OVMF/OVMF_VARS_4M.ms.fd"
            } else {
                "/usr/share/OVMF/OVMF_VARS_4M.fd"
            },
            "/work/OVMF_VARS.fd",
        )
        .map_err(|e| e.to_string())?;
    }
    let kvm = fs::OpenOptions::new().read(true).write(true).open("/dev/kvm").is_ok();
    let mut command = Command::new("/usr/bin/qemu-system-x86_64");
    command.args([
        "-machine",
        if secure_boot { "q35,smm=on" } else { "q35" },
        "-accel",
        if kvm { "kvm" } else { "tcg" },
        "-cpu",
        if kvm { "host" } else { "max" },
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
        if secure_boot {
            "if=pflash,format=raw,readonly=on,file=/usr/share/OVMF/OVMF_CODE_4M.secboot.fd"
        } else {
            "if=pflash,format=raw,readonly=on,file=/usr/share/OVMF/OVMF_CODE_4M.fd"
        },
        "-drive",
        "if=pflash,format=raw,file=/work/OVMF_VARS.fd",
        "-drive",
        "if=none,id=target,format=qcow2,cache=unsafe,file=/work/target.qcow2",
        "-device",
        "virtio-blk-pci,drive=target,serial=BOREA-TEST-TARGET",
        "-netdev",
        "user,id=net0",
        "-device",
        "virtio-net-pci,netdev=net0",
        "-device",
        "virtio-rng-pci",
    ]);
    if secure_boot {
        command.args(["-global", "driver=cfi.pflash01,property=secure,value=on"]);
    }
    if !boot {
        if secure_boot {
            media::secure_live_media()?;
            command.args([
                "-drive",
                "if=none,id=live,format=raw,file=fat:ro:/work/live-esp",
                "-device",
                "virtio-blk-pci,drive=live,bootindex=1",
            ]);
        } else {
            command.args([
                "-kernel",
                "/work/vmlinuz",
                "-initrd",
                "/work/live.cpio.gz",
                "-append",
                "console=ttyS0,115200 rdinit=/init panic=-1 tsc=reliable",
            ]);
        }
        command.args([
            "-fw_cfg",
            "name=opt/borea/autotest,string=BOREA_DISPOSABLE_VM",
            "-fw_cfg",
            &format!("name=opt/borea/distro,string={distro}"),
            "-fw_cfg",
            &format!("name=opt/borea/layout,string={layout}"),
            "-fw_cfg",
            &format!("name=opt/borea/secure-boot,string={}", u8::from(secure_boot)),
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
    let mut distro_ok = false;
    let mut user_ok = false;
    let mut preservation_ok = layout != "alongside";
    let mut firmware_ok = !secure_boot;
    let mut install_exit_ok = false;
    let expected_distro =
        if distro == "opensuse" { "opensuse-tumbleweed" } else { distro.as_str() };
    let firmware_check = if secure_boot {
        "test \"$(tail -c 1 /sys/firmware/efi/efivars/SecureBoot-8be4df61-93ca-11d2-aa0d-00e098032b8c | od -An -t u1 | tr -d ' ')\" = 1 && echo BOREA_SECURE_BOOT_OK; "
    } else {
        ""
    };
    let checks = format!(
        ". /etc/os-release; test \"$ID\" = {expected_distro} && echo BOREA_DISTRO_OK; test \"$(id -un)\" = boreatest && echo BOREA_USER_OK; test -b /dev/mapper/borea-root && echo BOREA_LUKS_OK; nm-online -q --timeout=60 && echo BOREA_NET_OK; {firmware_check}sudo -S -p '[BOREA_SUDO]' id -u\n"
    );
    loop {
        if start.elapsed() > Duration::from_secs(14400) {
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
                if buffer.contains("Kernel panic") || buffer.contains("BOREA_TEST_ERROR") {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err("The guest reported a fatal error; inspect its console log.".into());
                }
                if !boot {
                    complete |= buffer.contains("BOREA_INSTALL_COMPLETE");
                    preservation_ok |= buffer.contains("BOREA_PRESERVATION_OK");
                    install_exit_ok |= buffer.contains("BOREA_VM_INSTALL_EXIT_0");
                    continue;
                }
                distro_ok |= buffer.contains("BOREA_DISTRO_OK");
                user_ok |= buffer.contains("BOREA_USER_OK");
                firmware_ok |= buffer.contains("BOREA_SECURE_BOOT_OK");
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
                        Some(checks.as_str())
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
    if !status.success()
        || !complete
        || !boot && !(preservation_ok && install_exit_ok)
        || boot && !(luks && net && sudo && distro_ok && user_ok && firmware_ok)
    {
        return Err(format!(
            "VM assertion failed: exit={status}, complete={complete}, luks={luks}, network={net}, sudo={sudo}, firmware={firmware_ok}, preservation={preservation_ok}, installer_exit={install_exit_ok}"
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
