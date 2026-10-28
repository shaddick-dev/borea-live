//! `borea-build iso`: build Borea Live from upstream sources with Buildroot.
//!
//! No root, no distribution packages. The first build compiles the toolchain,
//! kernel, LLVM, Mesa, Rust, and the rest: hours, and about 30 GB. Later builds
//! reuse whatever has not changed.

use crate::run::{arg, command, on_path, output, prefer, sha256, tool};
use crate::{Result, keys};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

const BUILDROOT_VERSION: &str = "2026.08.1";
const BUILDROOT_SHA256: &str = "34fb7bea8a213ffde39cc02e61703391560f26e0850f03138c722d0b09880f2b";

/// Host programs Buildroot and the hooks call.
const HOST_TOOLS: &[&str] = &[
    "make", "gcc", "g++", "patch", "perl", "python3", "rsync", "cpio", "unzip", "bc", "wget",
    "file", "tar", "xz", "bzip2", "cargo", "curl",
];
const SIGNING_TOOLS: &[&str] = &["sbsign", "openssl"];

struct Options {
    configure_only: bool,
    secure_boot: Option<PathBuf>,
    jobs: Option<usize>,
}

fn options(args: &[&str]) -> Result<Options> {
    let mut options = Options { configure_only: false, secure_boot: None, jobs: None };
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        match *arg {
            "--configure-only" => options.configure_only = true,
            "--secure-boot" => {
                let dir = args.next().ok_or("--secure-boot needs the key directory")?;
                options.secure_boot = Some(PathBuf::from(dir));
            }
            "--jobs" => {
                let n = args.next().and_then(|n| n.parse().ok()).filter(|n| *n > 0);
                options.jobs = Some(n.ok_or("--jobs needs a number above zero")?);
            }
            other => return Err(format!("unknown option {other}")),
        }
    }
    Ok(options)
}

fn step(text: &str) {
    println!("\n\x1b[1m{text}\x1b[0m");
}

pub fn run(args: &[&str]) -> Result {
    let options = options(args)?;
    let repo = crate::repo()?;
    // As root, every file in target/live would end up root's, and the next
    // normal build would fail on them. Buildroot needs no root anyway.
    if rustix::process::geteuid().is_root() {
        return Err("run this as your normal user, not as root: the build needs no root".into());
    }
    let work =
        std::env::var_os("BOREA_LIVE_WORK").map_or_else(|| repo.join("target/live"), PathBuf::from);
    let buildroot = work.join(format!("buildroot-{BUILDROOT_VERSION}"));
    let output_dir = work.join("output");
    let external = repo.join("live/buildroot");
    for dir in ["dl", "src", "out", "hooks"] {
        fs::create_dir_all(work.join(dir)).map_err(|e| e.to_string())?;
    }
    // One build at a time per checkout: two would corrupt each other's output.
    let _lock = lock(&work)?;
    if let Some(pid) = make_running_in(&output_dir) {
        return Err(format!(
            "a Buildroot build is already running in {} (process {pid})",
            output_dir.display()
        ));
    }

    let mut wanted: Vec<&str> = HOST_TOOLS.to_vec();
    if options.secure_boot.is_some() {
        wanted.extend(SIGNING_TOOLS);
    }
    host_tools(&wanted)?;

    let mut env: Vec<(String, String)> = Vec::new();
    if let Some(dir) = &options.secure_boot {
        let (key, cert) = (absolute(&dir.join("db.key"))?, absolute(&dir.join("db.crt"))?);
        keys::check_pair(&key, &cert)?;
        step(&format!(
            "Secure Boot: signing with {}",
            output("openssl", &["x509", "-in", arg(&cert)?, "-noout", "-subject"])?.trim()
        ));
        env.push(("BOREA_SB_KEY".into(), arg(&key)?.into()));
        env.push(("BOREA_SB_CERT".into(), arg(&cert)?.into()));
    }

    fetch_buildroot(&work, &buildroot)?;
    link_hooks(&work)?;

    let version = workspace_version(&repo);
    env.push(("BOREA_LIVE_VERSION".into(), version.clone()));
    env.push(("BOREA_LIVE_BUILD_ID".into(), build_id()?));
    env.push(("BR2_DL_DIR".into(), arg(&work.join("dl"))?.into()));
    let jobs = options.jobs.unwrap_or_else(|| {
        let cpus = std::thread::available_parallelism().map_or(1, |n| n.get());
        default_jobs(cpus, memory_bytes())
    });
    let cpus = std::thread::available_parallelism().map_or(1, |n| n.get());
    // `parallel` builds independent packages side by side (top-level `make -j`,
    // with per-package directories). Make-based packages share those job slots;
    // ninja, meson and cargo ones (LLVM, Mesa) take BR2_JLEVEL each, so the load
    // limit keeps the machine from being oversubscribed while they overlap.
    let make = |targets: &[&str], parallel: bool| -> Result {
        // Signing variables come only from this run, never from the caller's shell.
        let mut make = command("make");
        if parallel {
            make.arg(format!("-j{jobs}")).arg(format!("--load-average={cpus}"));
        }
        let status = make
            .arg("-C")
            .arg(&buildroot)
            .arg(format!("O={}", output_dir.display()))
            .arg(format!("BR2_EXTERNAL={}", external.display()))
            // Overrides the defconfig's "one job per CPU", which runs out of memory
            // compiling LLVM on machines with many threads and little RAM.
            .arg(format!("BR2_JLEVEL={jobs}"))
            .args(targets)
            .env_remove("BOREA_SB_KEY")
            .env_remove("BOREA_SB_CERT")
            .envs(env.iter().map(|(k, v)| (k, v)))
            .status()
            .map_err(|e| format!("could not run make: {e}"))?;
        status
            .success()
            .then_some(())
            .ok_or_else(|| format!("make {} failed ({status})", targets.join(" ")))
    };

    step("Configuring");
    make(&["borea_live_defconfig"], false)?;
    let dropped = dropped_options(
        &fs::read_to_string(external.join("configs/borea_live_defconfig"))
            .map_err(|e| e.to_string())?,
        &fs::read_to_string(output_dir.join(".config")).map_err(|e| e.to_string())?,
    );
    if !dropped.is_empty() {
        return Err(format!(
            "Buildroot did not accept these options (renamed, or unmet dependencies):\n  {}",
            dropped.join("\n  ")
        ));
    }
    println!("All Buildroot {BUILDROOT_VERSION} options accepted.");

    // Buildroot never rebuilds a package whose options changed (a new sub-option
    // like gnupg2's gpgv would never appear). Configuring kept the previous
    // configuration as .config.old, so rebuild exactly those packages.
    if let (Ok(old), Ok(new)) = (
        fs::read_to_string(output_dir.join(".config.old")),
        fs::read_to_string(output_dir.join(".config")),
    ) {
        let known = package_names(&[buildroot.join("package"), external.join("package")]);
        let changed = changed_packages(&old, &new, |name| known.contains(name));
        if !changed.is_empty() {
            step(&format!("Options changed for {}: rebuilding them", changed.join(", ")));
            let targets: Vec<String> = changed.iter().map(|p| format!("{p}-dirclean")).collect();
            make(&targets.iter().map(String::as_str).collect::<Vec<_>>(), false)?;
        }
    }

    if options.configure_only {
        step(&format!("Configured: {}", output_dir.join(".config").display()));
        return Ok(());
    }

    step(&format!(
        "Building with {jobs} parallel jobs, several packages at once (the first build can take up to a few hours, depending on your machine; later ones reuse what is unchanged)"
    ));
    // Buildroot needs a clean build after the toolchain or directory layout
    // changes, and does not notice by itself. Downloads and ccache survive.
    let layout = build_layout(
        &fs::read_to_string(external.join("configs/borea_live_defconfig"))
            .map_err(|e| e.to_string())?,
    );
    let stamp = output_dir.join(".borea-layout");
    if fs::read_to_string(&stamp).is_ok_and(|built| built != layout)
        || (!stamp.exists() && output_dir.join("build").exists())
    {
        step("The toolchain or build layout changed: starting a clean build");
        make(&["clean"], false)?;
        let _ = fs::remove_dir_all(output_dir.join("borea-cargo"));
    }
    fs::write(&stamp, &layout).map_err(|e| e.to_string())?;
    let source = pack_source(&repo, &work)?;
    let installed_source = output_dir.join(".borea-source");
    if fs::read_to_string(&installed_source).ok().as_deref() != Some(&source)
        || !output_dir.join("target/usr/bin/borea-live").is_file()
    {
        make(&["borea-dirclean"], false)?;
    }
    make(&[], true)?;
    fs::write(installed_source, source).map_err(|e| e.to_string())?;

    let date = output("date", &["+%Y%m%d"])?;
    let signed = if options.secure_boot.is_some() { "-signed" } else { "" };
    let name = format!("borea-live-{version}-{}{signed}.iso", date.trim());
    let iso = work.join("out").join(&name);
    fs::copy(output_dir.join("images/borea-live.iso"), &iso).map_err(|e| e.to_string())?;
    fs::write(
        work.join("out").join(format!("{name}.sha256")),
        format!("{}  {name}\n", sha256(&iso)?),
    )
    .map_err(|e| e.to_string())?;
    step(&format!("Done: {}", iso.display()));
    println!("Try it:   qemu-system-x86_64 -enable-kvm -m 4G -cdrom {}", iso.display());
    println!(
        "Write it: borea-flash   (finds the newest build itself; install it once with `cargo run --profile app -p borea-flash -- --install`)"
    );
    Ok(())
}

/// Every package Buildroot (and the Borea external tree) can build, by name:
/// each `<name>/<name>.mk`, however deep in `package/`.
fn package_names(roots: &[PathBuf]) -> std::collections::HashSet<String> {
    fn walk(dir: &Path, found: &mut std::collections::HashSet<String>) {
        let Ok(entries) = fs::read_dir(dir) else { return };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                if let Some(name) = path.file_name().and_then(|n| n.to_str())
                    && path.join(format!("{name}.mk")).is_file()
                {
                    found.insert(name.to_owned());
                }
                walk(&path, found);
            }
        }
    }
    let mut found = std::collections::HashSet::new();
    for root in roots {
        walk(root, &mut found);
    }
    found
}

/// Buildroot packages whose `BR2_PACKAGE_*` options differ between two
/// configurations, as make target names (`gnupg2`, `host-lld`). Each changed
/// symbol belongs to the longest package name it starts with.
fn changed_packages(old: &str, new: &str, exists: impl Fn(&str) -> bool) -> Vec<String> {
    fn symbols(config: &str) -> std::collections::BTreeMap<&str, &str> {
        config
            .lines()
            .filter_map(|line| {
                if let Some(rest) =
                    line.strip_prefix("# ").and_then(|l| l.strip_suffix(" is not set"))
                {
                    return Some((rest, "n"));
                }
                line.split_once('=')
            })
            .filter(|(symbol, _)| symbol.starts_with("BR2_PACKAGE_"))
            .collect()
    }
    let (old, new) = (symbols(old), symbols(new));
    let mut changed = std::collections::BTreeSet::new();
    for symbol in old.keys().chain(new.keys()) {
        if old.get(symbol).unwrap_or(&"n") == new.get(symbol).unwrap_or(&"n") {
            continue;
        }
        let rest = &symbol["BR2_PACKAGE_".len()..];
        let (host, rest) = match rest.strip_prefix("HOST_") {
            Some(rest) => (true, rest),
            None => (false, rest),
        };
        let words: Vec<&str> = rest.split('_').collect();
        for n in (1..=words.len()).rev() {
            let name = words[..n].join("-").to_lowercase();
            if exists(&name) {
                changed.insert(if host { format!("host-{name}") } else { name });
                break;
            }
        }
    }
    changed.into_iter().collect()
}

/// The defconfig lines that need a clean build when they change.
fn build_layout(defconfig: &str) -> String {
    defconfig
        .lines()
        .filter(|l| l.starts_with("BR2_TOOLCHAIN") || l.starts_with("BR2_PER_PACKAGE"))
        .map(|l| format!("{l}\n"))
        .collect()
}

/// Memory each compile job may need: LLVM's larger C++ files take 1–2 GB each.
const MEMORY_PER_JOB: u64 = 2 << 30;

/// One job per CPU, but no more than the memory holds.
fn default_jobs(cpus: usize, memory: Option<u64>) -> usize {
    let fits = memory.map_or(cpus, |bytes| (bytes / MEMORY_PER_JOB) as usize);
    cpus.min(fits).max(1)
}

/// The machine's total memory, from `/proc/meminfo`.
fn memory_bytes() -> Option<u64> {
    let info = fs::read_to_string("/proc/meminfo").ok()?;
    let line = info.lines().find(|l| l.starts_with("MemTotal:"))?;
    let kib: u64 = line.split_whitespace().nth(1)?.parse().ok()?;
    Some(kib * 1024)
}

/// Held for the whole build; released when the process ends, however it ends.
fn lock(work: &Path) -> Result<fs::File> {
    let file = fs::File::create(work.join(".build.lock")).map_err(|e| e.to_string())?;
    rustix::fs::flock(&file, rustix::fs::FlockOperation::NonBlockingLockExclusive)
        .map_err(|_| "another Borea Live build is already running in this checkout".to_owned())?;
    Ok(file)
}

/// A `make` already building into `output`, started by anything (including builds
/// from before the lock existed).
fn make_running_in(output: &Path) -> Option<u32> {
    let wanted = format!("O={}", output.display());
    fs::read_dir("/proc").ok()?.filter_map(|e| e.ok()).find_map(|entry| {
        let pid: u32 = entry.file_name().to_str()?.parse().ok()?;
        let cmdline = fs::read(entry.path().join("cmdline")).ok()?;
        let mut args = cmdline.split(|b| *b == 0).map(String::from_utf8_lossy);
        let program = args.next()?;
        (program.ends_with("make") && args.any(|a| a == wanted)).then_some(pid)
    })
}

fn absolute(path: &Path) -> Result<PathBuf> {
    fs::canonicalize(path).map_err(|e| format!("{}: {e}", path.display()))
}

/// Missing tools come from Nix for this build only, when Nix is installed. Nothing
/// is installed system-wide.
fn host_tools(wanted: &[&str]) -> Result {
    let missing = |list: &[&str]| -> Vec<String> {
        list.iter().filter(|t| !on_path(t)).map(|t| t.to_string()).collect()
    };
    let lacking = missing(wanted);
    if !lacking.is_empty() && on_path("nix") {
        step(&format!("Borrowing {} from Nix for this build", lacking.join(" ")));
        let borrowed = lacking
            .iter()
            .filter_map(|tool| {
                let package = if tool == "sbsign" { "sbsigntool" } else { tool };
                let out = command("nix")
                    .args([
                        "--extra-experimental-features",
                        "nix-command flakes",
                        "build",
                        "--no-link",
                        "--print-out-paths",
                    ])
                    .arg(format!("nixpkgs#{package}"))
                    .output()
                    .ok()?;
                let store = String::from_utf8_lossy(&out.stdout).lines().last()?.to_owned();
                Some(PathBuf::from(store).join("bin"))
            })
            .collect();
        prefer(borrowed);
    }
    let still = missing(wanted);
    still.is_empty().then_some(()).ok_or_else(|| format!("missing host tools: {}", still.join(" ")))
}

fn fetch_buildroot(work: &Path, buildroot: &Path) -> Result {
    if buildroot.is_dir() {
        return Ok(());
    }
    step(&format!("Fetching Buildroot {BUILDROOT_VERSION}"));
    let tarball = work.join(format!("buildroot-{BUILDROOT_VERSION}.tar.xz"));
    if !tarball.is_file() {
        let url = format!("https://buildroot.org/downloads/buildroot-{BUILDROOT_VERSION}.tar.xz");
        tool("curl", &["-fL", "--retry", "3", "-o", arg(&tarball)?, &url])?;
    }
    let actual = sha256(&tarball)?;
    if actual != BUILDROOT_SHA256 {
        return Err(format!(
            "Buildroot download does not match its published SHA-256 (got {actual})"
        ));
    }
    tool("tar", &["-xf", arg(&tarball)?, "-C", arg(work)?])
}

/// This workspace with every Rust dependency vendored, so Buildroot builds offline.
const SOURCE_ITEMS: &[&str] = &["Cargo.toml", "Cargo.lock", "rustfmt.toml", "crates", "backend"];

/// Content identity rather than mtimes: a checkout or touch must not force a rebuild.
fn source_fingerprint(repo: &Path, manifests_only: bool) -> Result<String> {
    fn visit(root: &Path, path: &Path, digest: &mut Sha256, manifests_only: bool) -> Result {
        let meta = fs::symlink_metadata(path).map_err(|e| format!("{}: {e}", path.display()))?;
        if meta.is_symlink() {
            return Err(format!(
                "source input {} is a symlink; use files inside the checkout",
                path.display()
            ));
        }
        if meta.is_dir() {
            let mut children = fs::read_dir(path)
                .map_err(|e| e.to_string())?
                .map(|entry| entry.map(|entry| entry.path()))
                .collect::<std::io::Result<Vec<_>>>()
                .map_err(|e| e.to_string())?;
            children.sort();
            for child in children {
                if child.file_name().is_some_and(|name| name == "target") {
                    continue;
                }
                visit(root, &child, digest, manifests_only)?;
            }
        } else if meta.is_file()
            && (!manifests_only
                || path
                    .file_name()
                    .is_some_and(|name| name == "Cargo.toml" || name == "Cargo.lock"))
        {
            let relative =
                path.strip_prefix(root).map_err(|e| e.to_string())?.as_os_str().as_encoded_bytes();
            digest.update((relative.len() as u64).to_le_bytes());
            digest.update(relative);
            digest.update(meta.len().to_le_bytes());
            let mut file = fs::File::open(path).map_err(|e| e.to_string())?;
            let mut buffer = [0_u8; 64 * 1024];
            loop {
                let length = file.read(&mut buffer).map_err(|e| e.to_string())?;
                if length == 0 {
                    break;
                }
                digest.update(&buffer[..length]);
            }
        }
        Ok(())
    }
    let mut digest = Sha256::new();
    for item in SOURCE_ITEMS {
        visit(repo, &repo.join(item), &mut digest, manifests_only)?;
    }
    if !manifests_only {
        visit(repo, &repo.join("live/buildroot/package/borea/borea.mk"), &mut digest, false)?;
    }
    Ok(digest.finalize().iter().map(|byte| format!("{byte:02x}")).collect())
}

fn source_archive_matches(archive: &Path, stamp: &Path, source: &str) -> bool {
    let Ok(saved) = fs::read_to_string(stamp) else {
        return false;
    };
    let mut lines = saved.lines();
    lines.next() == Some(source)
        && lines
            .next()
            .is_some_and(|expected| sha256(archive).is_ok_and(|actual| actual == expected))
}

fn pack_source(repo: &Path, work: &Path) -> Result<String> {
    let source = source_fingerprint(repo, false)?;
    let archive_stamp = work.join("src/source.sha256");
    let archive = work.join("src/borea-local.tar.gz");
    if source_archive_matches(&archive, &archive_stamp, &source) {
        step("Reusing Borea's unchanged source archive and vendored dependencies");
        return Ok(source);
    }
    step("Packing Borea's source");
    let staging = work.join("src-staging");
    let tree = staging.join("borea-local");
    for item in SOURCE_ITEMS {
        let old = tree.join(item);
        if old.is_dir() {
            fs::remove_dir_all(&old).map_err(|e| e.to_string())?;
        } else if old.exists() {
            fs::remove_file(&old).map_err(|e| e.to_string())?;
        }
        copy_tree(&repo.join(item), &tree.join(item))?;
    }
    let vendor = tree.join("VENDOR");
    let manifests = source_fingerprint(repo, true)?;
    let vendor_stamp = work.join("src/vendor.sha256");
    if !vendor.is_dir() || fs::read_to_string(&vendor_stamp).ok().as_deref() != Some(&manifests) {
        if vendor.exists() {
            fs::remove_dir_all(&vendor).map_err(|e| e.to_string())?;
        }
        tool(
            "cargo",
            &[
                "vendor",
                "--quiet",
                "--locked",
                "--manifest-path",
                arg(&tree.join("Cargo.toml"))?,
                arg(&vendor)?,
            ],
        )?;
        fs::write(vendor_stamp, manifests).map_err(|e| e.to_string())?;
    }
    // Written here rather than taken from `cargo vendor`'s output, which `--quiet`
    // leaves empty. Every dependency comes from crates.io, so one source suffices.
    fs::create_dir_all(tree.join(".cargo")).map_err(|e| e.to_string())?;
    fs::write(tree.join(".cargo/config.toml"), VENDOR_CONFIG).map_err(|e| e.to_string())?;
    let pending_archive = work.join("src/borea-local.tar.gz.new");
    tool("tar", &["-czf", arg(&pending_archive)?, "-C", arg(&staging)?, "borea-local"])?;
    fs::rename(pending_archive, &archive).map_err(|e| e.to_string())?;
    // Make Buildroot take the fresh tarball rather than a cached copy.
    let cached = work.join("dl/borea");
    if cached.exists() {
        fs::remove_dir_all(cached).map_err(|e| e.to_string())?;
    }
    fs::write(archive_stamp, format!("{source}\n{}\n", sha256(&archive)?))
        .map_err(|e| e.to_string())?;
    Ok(source)
}

/// Points cargo at the vendored crates, relative to the packed source tree.
const VENDOR_CONFIG: &str = "[source.crates-io]\nreplace-with = \"vendored-sources\"\n\n[source.vendored-sources]\ndirectory = \"VENDOR\"\n";

fn copy_tree(from: &Path, to: &Path) -> Result {
    let meta = fs::metadata(from).map_err(|e| format!("{}: {e}", from.display()))?;
    if meta.is_dir() {
        fs::create_dir_all(to).map_err(|e| e.to_string())?;
        for entry in fs::read_dir(from).map_err(|e| e.to_string())? {
            let entry = entry.map_err(|e| e.to_string())?;
            if entry.file_name() == "target" {
                continue;
            }
            copy_tree(&entry.path(), &to.join(entry.file_name()))?;
        }
        Ok(())
    } else {
        if let Some(parent) = to.parent() {
            fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
        }
        fs::copy(from, to)
            .map(drop)
            .map_err(|e| format!("{} → {}: {e}", from.display(), to.display()))
    }
}

/// Buildroot runs `target/live/hooks/post-build` and `post-image`: both are this
/// binary, which acts on the name it is started as.
fn link_hooks(work: &Path) -> Result {
    let me = std::env::current_exe().map_err(|e| e.to_string())?;
    for hook in ["post-build", "post-image"] {
        let link = work.join("hooks").join(hook);
        let _ = fs::remove_file(&link);
        std::os::unix::fs::symlink(&me, &link).map_err(|e| format!("{}: {e}", link.display()))?;
    }
    Ok(())
}

fn workspace_version(repo: &Path) -> String {
    fs::read_to_string(repo.join("Cargo.toml"))
        .ok()
        .and_then(|text| {
            text.lines().find_map(|l| {
                l.strip_prefix("version = \"").and_then(|v| v.strip_suffix('"')).map(str::to_owned)
            })
        })
        .unwrap_or_else(|| "dev".into())
}

/// A random ID for this build; the early boot uses it to find its own medium.
fn build_id() -> Result<String> {
    let mut bytes = [0u8; 16];
    fs::File::open("/dev/urandom")
        .and_then(|mut f| f.read_exact(&mut bytes))
        .map_err(|e| e.to_string())?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

/// Options set in the defconfig that Buildroot's resolved `.config` lacks.
fn dropped_options(defconfig: &str, config: &str) -> Vec<String> {
    let resolved: BTreeSet<&str> = config.lines().collect();
    defconfig
        .lines()
        .map(str::trim)
        .filter(|l| l.starts_with("BR2_") && !l.contains("=\""))
        .filter(|l| !resolved.contains(l))
        .map(str::to_owned)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_and_vendor_identity_follow_content_and_manifest_changes() {
        let dir = std::env::temp_dir().join(format!("borea-source-key-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        for path in [
            "Cargo.toml",
            "Cargo.lock",
            "rustfmt.toml",
            "crates/app/src/main.rs",
            "crates/app/Cargo.toml",
            "backend/test.rs",
            "live/buildroot/package/borea/borea.mk",
        ] {
            let path = dir.join(path);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, "original").unwrap();
        }
        let source = source_fingerprint(&dir, false).unwrap();
        let vendor = source_fingerprint(&dir, true).unwrap();
        fs::write(dir.join("crates/app/src/main.rs"), "new implementation").unwrap();
        assert_ne!(source, source_fingerprint(&dir, false).unwrap());
        assert_eq!(vendor, source_fingerprint(&dir, true).unwrap());
        fs::write(dir.join("Cargo.lock"), "new dependency").unwrap();
        assert_ne!(vendor, source_fingerprint(&dir, true).unwrap());
        fs::remove_file(dir.join("backend/test.rs")).unwrap();
        assert_ne!(source, source_fingerprint(&dir, false).unwrap());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn interrupted_or_corrupted_source_archives_are_never_reused() {
        let dir = std::env::temp_dir().join(format!("borea-archive-key-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let archive = dir.join("source.tar.gz");
        let stamp = dir.join("stamp");
        fs::write(&archive, "archive").unwrap();
        fs::write(&stamp, format!("source-key\n{}\n", sha256(&archive).unwrap())).unwrap();
        assert!(source_archive_matches(&archive, &stamp, "source-key"));
        assert!(!source_archive_matches(&archive, &stamp, "different-source"));
        fs::write(&archive, "incomplete").unwrap();
        assert!(!source_archive_matches(&archive, &stamp, "source-key"));
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn dropped_options_are_caught() {
        let defconfig = "# comment\nBR2_PACKAGE_CAGE=y\nBR2_PACKAGE_GONE=y\nBR2_TARGET_GENERIC_HOSTNAME=\"borea-live\"\n";
        let config = "BR2_PACKAGE_CAGE=y\n# BR2_PACKAGE_GONE is not set\nBR2_TARGET_GENERIC_HOSTNAME=\"x\"\n";
        assert_eq!(dropped_options(defconfig, config), ["BR2_PACKAGE_GONE=y"]);
    }

    #[test]
    fn packages_whose_options_changed_are_found() {
        let old = "BR2_PACKAGE_GNUPG2=y\n# BR2_PACKAGE_GNUPG2_GPGV is not set\nBR2_PACKAGE_IW=y\n# BR2_PACKAGE_HOST_LLD is not set\nBR2_PACKAGE_LINUX_FIRMWARE=y\n";
        let new = "BR2_PACKAGE_GNUPG2=y\nBR2_PACKAGE_GNUPG2_GPGV=y\nBR2_PACKAGE_IW=y\nBR2_PACKAGE_HOST_LLD=y\nBR2_PACKAGE_LINUX_FIRMWARE=y\nBR2_PACKAGE_LINUX_FIRMWARE_IWLWIFI_7=y\nBR2_TOOLCHAIN_EXTERNAL=y\n";
        let known = ["gnupg2", "iw", "lld", "linux-firmware", "linux"];
        let changed = changed_packages(old, new, |name| known.contains(&name));
        assert_eq!(changed, ["gnupg2", "host-lld", "linux-firmware"]);
        assert!(changed_packages(new, new, |_| true).is_empty());
    }

    #[test]
    fn layout_tracks_only_what_needs_a_clean_build() {
        let a = "BR2_x86_64=y\nBR2_TOOLCHAIN_EXTERNAL=y\nBR2_PER_PACKAGE_DIRECTORIES=y\nBR2_PACKAGE_IW=y\n";
        let b = "BR2_x86_64=y\nBR2_TOOLCHAIN_EXTERNAL=y\nBR2_PER_PACKAGE_DIRECTORIES=y\n";
        assert_eq!(build_layout(a), build_layout(b), "adding a package needs no clean build");
        let c = "BR2_TOOLCHAIN_BUILDROOT_GLIBC=y\n";
        assert_ne!(build_layout(a), build_layout(c));
        assert!(build_layout(a).contains("BR2_PER_PACKAGE_DIRECTORIES=y"));
    }

    #[test]
    fn options_parse() {
        let o = options(&["--configure-only", "--secure-boot", "/keys"]).unwrap();
        assert!(o.configure_only);
        assert_eq!(o.secure_boot.as_deref(), Some(Path::new("/keys")));
        assert!(options(&["--secure-boot"]).is_err());
        assert!(options(&["--fast"]).is_err());
        assert_eq!(options(&["--jobs", "4"]).unwrap().jobs, Some(4));
        assert!(options(&["--jobs", "0"]).is_err());
        assert!(options(&["--jobs"]).is_err());
    }

    #[test]
    fn jobs_fit_the_memory() {
        let gib = |n: u64| Some(n << 30);
        // This laptop: 20 threads, 15.2 GiB.
        assert_eq!(default_jobs(20, Some(15_975_040 * 1024)), 7);
        assert_eq!(default_jobs(4, gib(64)), 4, "CPUs limit a big-memory machine");
        assert_eq!(default_jobs(8, gib(1)), 1, "always at least one");
        assert_eq!(default_jobs(6, None), 6, "unknown memory: one per CPU");
    }

    /// The packed source builds offline: cargo resolves every locked crate from
    /// VENDOR alone (as it did not when the config came out empty).
    #[test]
    fn vendored_sources_resolve_offline() {
        let lock = include_str!("../../../Cargo.lock");
        assert!(
            lock.lines()
                .filter(|l| l.starts_with("source = "))
                .all(|l| l.contains("registry+https://github.com/rust-lang/crates.io-index")),
            "a non-crates.io source needs its own entry in VENDOR_CONFIG"
        );
        let config: toml::Table = toml::from_str(VENDOR_CONFIG).unwrap();
        assert_eq!(
            config["source"]["crates-io"]["replace-with"].as_str(),
            Some("vendored-sources")
        );
        assert_eq!(config["source"]["vendored-sources"]["directory"].as_str(), Some("VENDOR"));
    }

    #[test]
    fn build_ids_are_random_hex() {
        let (a, b) = (build_id().unwrap(), build_id().unwrap());
        assert_eq!(a.len(), 32);
        assert_ne!(a, b);
    }
}
