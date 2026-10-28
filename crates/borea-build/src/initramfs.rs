//! The early boot image: `borea-init` as `/init`, the shared libraries it needs,
//! and a few small files, packed as a gzip-compressed `newc` cpio archive.

use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};

use flate2::Compression;
use flate2::write::GzEncoder;
use goblin::elf::Elf;

use crate::Result;

/// One archive member.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Entry {
    Dir,
    File {
        mode: u32,
        data: Vec<u8>,
    },
    /// A character device such as `/dev/console`.
    CharDevice {
        major: u32,
        minor: u32,
    },
    Symlink(String),
}

/// An early boot image under construction, keyed by absolute path.
#[derive(Debug, Default)]
pub struct Image {
    entries: BTreeMap<String, Entry>,
}

impl Image {
    pub fn new() -> Self {
        let mut image = Image::default();
        // The kernel opens /dev/console for /init before anything is mounted;
        // without it, early boot messages go nowhere.
        image.add(PathBuf::from("/dev/console"), Entry::CharDevice { major: 5, minor: 1 });
        // Every library lives in /lib; loaders that look in /lib64 find them too.
        image.add(PathBuf::from("/lib64"), Entry::Symlink("lib".into()));
        image
    }

    fn add(&mut self, path: PathBuf, entry: Entry) {
        let mut parent = path.parent();
        while let Some(dir) = parent.filter(|d| *d != Path::new("/")) {
            self.entries.entry(dir.to_string_lossy().into_owned()).or_insert(Entry::Dir);
            parent = dir.parent();
        }
        self.entries.insert(path.to_string_lossy().into_owned(), entry);
    }

    pub fn file(&mut self, path: &str, mode: u32, data: Vec<u8>) {
        self.add(PathBuf::from(path), Entry::File { mode, data });
    }

    /// Add a program from `root` (the target's root filesystem) at `at`, with its
    /// ELF interpreter and every shared library it needs, recursively.
    pub fn program(&mut self, root: &Path, from: &str, at: &str) -> Result {
        let data = std::fs::read(root.join(from.trim_start_matches('/')))
            .map_err(|e| format!("{from}: {e}"))?;
        let needs = elf_needs(&data)?;
        self.file(at, 0o755, data);
        for needed in needs {
            self.library(root, &needed)?;
        }
        Ok(())
    }

    fn library(&mut self, root: &Path, name: &str) -> Result {
        // An interpreter is an absolute path; libraries are names to look up.
        let (source, at) = if name.starts_with('/') {
            let file = Path::new(name)
                .file_name()
                .map(|f| f.to_string_lossy().into_owned())
                .unwrap_or_default();
            (root.join(name.trim_start_matches('/')), format!("/lib/{file}"))
        } else {
            let dir = [
                "usr/lib",
                "lib",
                "usr/lib64",
                "lib64",
                "usr/lib/x86_64-linux-gnu",
                "lib/x86_64-linux-gnu",
            ]
            .into_iter()
            .find(|dir| root.join(dir).join(name).exists())
            .ok_or_else(|| format!("{name} is not in the target's libraries"))?;
            (root.join(dir).join(name), format!("/lib/{name}"))
        };
        if self.entries.contains_key(&at) {
            return Ok(());
        }
        // Follow symlinks: the archive holds the library itself under its soname.
        let data = std::fs::read(&source).map_err(|e| format!("{}: {e}", source.display()))?;
        let needs = elf_needs(&data)?;
        self.file(&at, 0o755, data);
        for needed in needs {
            self.library(root, &needed)?;
        }
        Ok(())
    }

    /// Prebuilt kernels keep storage/filesystem drivers as modules. Buildroot's
    /// kernel has them built in and needs no module manifest.
    pub fn kernel_modules(&mut self, root: &Path) -> Result {
        let manifest = root.join("etc/borea-early-modules");
        if !manifest.is_file() {
            return Ok(());
        }
        let text = std::fs::read_to_string(manifest).map_err(|e| e.to_string())?;
        let mut lines = text.lines();
        let version =
            lines.next().filter(|s| !s.is_empty()).ok_or("missing early-boot kernel version")?;
        if version.contains('/') || version == "." || version == ".." {
            return Err("invalid early-boot kernel version".into());
        }
        let modules = lines.collect::<Vec<_>>();
        if modules.iter().any(|module| {
            module.is_empty() || !module.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
        }) {
            return Err("invalid early-boot module name".into());
        }
        self.program(root, "/usr/sbin/modprobe", "/sbin/modprobe")?;
        self.program(root, "/usr/sbin/losetup", "/sbin/losetup")?;
        let directory = root.join("usr/lib/modules").join(version);
        for file in std::fs::read_dir(&directory).map_err(|e| e.to_string())?.flatten() {
            let name = file.file_name().to_string_lossy().into_owned();
            if name.starts_with("modules.") && file.path().is_file() {
                self.file(
                    &format!("/lib/modules/{version}/{name}"),
                    0o644,
                    std::fs::read(file.path()).map_err(|e| e.to_string())?,
                );
            }
        }
        let mut boot = Vec::new();
        for module in modules {
            let mut cmd = crate::run::command("modprobe");
            let output = cmd
                .args([
                    "--dirname",
                    root.to_str().ok_or("invalid root path")?,
                    "--set-version",
                    version,
                    "--show-depends",
                    module,
                ])
                .output()
                .map_err(|e| e.to_string())?;
            if !output.status.success() {
                return Err(format!("cannot resolve early-boot module {module}"));
            }
            for line in String::from_utf8_lossy(&output.stdout).lines() {
                if let Some(path) =
                    line.strip_prefix("insmod ").and_then(|s| s.split_whitespace().next())
                {
                    let relative = Path::new(path)
                        .strip_prefix(&directory)
                        .or_else(|_| {
                            Path::new(path).strip_prefix(root.join("lib/modules").join(version))
                        })
                        .map_err(|_| "module path escaped the kernel directory")?;
                    self.file(
                        &format!("/lib/modules/{version}/{}", relative.display()),
                        0o644,
                        std::fs::read(path).map_err(|e| e.to_string())?,
                    );
                }
            }
            boot.push(module);
        }
        self.file("/boot-modules", 0o644, boot.join("\n").into_bytes());
        Ok(())
    }

    /// The archive, gzip-compressed, ready for the kernel.
    pub fn pack(&self) -> Result<Vec<u8>> {
        let mut gz = GzEncoder::new(Vec::new(), Compression::best());
        gz.write_all(&self.cpio()).and_then(|()| gz.finish()).map_err(|e| e.to_string())
    }

    /// The uncompressed `newc` archive: entries in path order, then the trailer.
    fn cpio(&self) -> Vec<u8> {
        let mut out = Vec::new();
        for (ino, (path, entry)) in self.entries.iter().enumerate() {
            let (mode, data, rdev): (u32, &[u8], (u32, u32)) = match entry {
                Entry::Dir => (0o040_755, &[], (0, 0)),
                Entry::File { mode, data } => (0o100_000 | mode, data, (0, 0)),
                Entry::CharDevice { major, minor } => (0o020_600, &[], (*major, *minor)),
                Entry::Symlink(target) => (0o120_777, target.as_bytes(), (0, 0)),
            };
            write_member(&mut out, ino as u32 + 1, mode, path.trim_start_matches('/'), data, rdev);
        }
        write_member(&mut out, 0, 0, "TRAILER!!!", &[], (0, 0));
        out
    }
}

fn write_member(out: &mut Vec<u8>, ino: u32, mode: u32, name: &str, data: &[u8], rdev: (u32, u32)) {
    let nlink = if mode & 0o170_000 == 0o040_000 { 2 } else { 1 };
    let fields = [
        ino,
        mode,
        0,
        0,
        nlink,
        0,
        data.len() as u32,
        0,
        0,
        rdev.0,
        rdev.1,
        name.len() as u32 + 1,
        0,
    ];
    out.extend_from_slice(b"070701");
    for field in fields {
        out.extend_from_slice(format!("{field:08x}").as_bytes());
    }
    out.extend_from_slice(name.as_bytes());
    out.push(0);
    pad(out);
    out.extend_from_slice(data);
    pad(out);
}

/// newc aligns every header and every file body to 4 bytes.
fn pad(out: &mut Vec<u8>) {
    while !out.len().is_multiple_of(4) {
        out.push(0);
    }
}

/// An ELF file's interpreter (absolute path) and needed libraries (sonames).
fn elf_needs(data: &[u8]) -> Result<Vec<String>> {
    let elf = Elf::parse(data).map_err(|e| format!("not an ELF file: {e}"))?;
    Ok(elf
        .interpreter
        .map(str::to_owned)
        .into_iter()
        .chain(elf.libraries.iter().map(|l| l.to_string()))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn early_module_manifest_rejects_paths_before_reading_programs() {
        let root =
            std::env::temp_dir().join(format!("borea-module-manifest-{}", std::process::id()));
        std::fs::create_dir_all(root.join("etc")).unwrap();
        for text in ["../kernel\nerofs\n", "6.12\n../driver\n", "6.12\n\n"] {
            std::fs::write(root.join("etc/borea-early-modules"), text).unwrap();
            let error = Image::new().kernel_modules(&root).unwrap_err();
            assert!(error.contains("invalid early-boot"), "{error}");
        }
        std::fs::remove_file(root.join("etc/borea-early-modules")).unwrap();
        assert!(
            Image::new().kernel_modules(&root).is_ok(),
            "Buildroot roots need no module manifest"
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn newc_layout() {
        let mut image = Image::new();
        image.file("/build-id", 0o644, b"abc\n".to_vec());
        let cpio = image.cpio();
        assert!(cpio.starts_with(b"070701"));
        assert_eq!(cpio.len() % 4, 0, "the archive is 4-byte aligned");
        let text = String::from_utf8_lossy(&cpio);
        assert!(
            text.contains("dev/console")
                && text.contains("build-id")
                && text.ends_with("TRAILER!!!\0\0\0\0")
        );
        // Parent directories are created before their contents.
        assert!(text.find("dev\0").unwrap() < text.find("dev/console").unwrap());
    }

    #[test]
    fn real_programs_bring_their_libraries() {
        // This machine's own /usr/bin/true is a dynamically linked ELF program. The
        // resolver searches the Buildroot target layout (/lib, /usr/lib), so the
        // host must share it; Debian-family multiarch hosts (and CI runners) do not.
        if !Path::new("/usr/lib/libc.so.6").exists() && !Path::new("/lib/libc.so.6").exists() {
            eprintln!("skipped: host libc is not in /lib or /usr/lib");
            return;
        }
        let mut image = Image::new();
        image.program(Path::new("/"), "/usr/bin/true", "/init").unwrap();
        let names: Vec<_> = image.entries.keys().cloned().collect();
        assert!(names.iter().any(|n| n.starts_with("/lib/libc.so")), "{names:?}");
        assert!(names.iter().any(|n| n.starts_with("/lib/ld-linux")), "{names:?}");
        assert_eq!(image.entries["/lib64"], Entry::Symlink("lib".into()));
        assert!(!image.pack().unwrap().is_empty());
    }
}
