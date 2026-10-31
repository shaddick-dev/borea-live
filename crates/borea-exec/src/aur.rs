//! One reviewed AUR builder for installation and quiet first-login work.
use borea_backend::external::{AurRecipe, identifier};
use borea_core::{Hint, hint};
use sha2::{Digest, Sha256};
use std::{fs, path::Path};

fn err(message: impl Into<String>) -> Hint {
    hint("aur", message)
}
fn args(values: &[&str]) -> Vec<String> {
    values.iter().map(|s| (*s).into()).collect()
}
pub(crate) fn valid_recipe(recipe: &AurRecipe) -> bool {
    identifier(&recipe.base)
        && !recipe.packages.is_empty()
        && recipe.packages.len() <= 128
        && recipe.packages.iter().all(|p| identifier(p))
        && recipe.dependencies.len() <= 512
        && recipe.dependencies.iter().all(|d| {
            d.len() < 256
                && d.bytes().all(|b| b.is_ascii_graphic())
                && identifier(d.split(['<', '>', '=']).next().unwrap_or(""))
        })
        && recipe.sha256.len() == 64
        && recipe.sha256.bytes().all(|b| b.is_ascii_hexdigit())
        && recipe.url
            == format!("https://aur.archlinux.org/cgit/aur.git/snapshot/{}.tar.gz", recipe.base)
}

// Locally built AUR packages have no repository signature. Limit that exception
// to these inspected local artifacts; keep the real config and remote policy intact.
fn local_package_config(root: &Path) -> Result<String, Hint> {
    let original =
        fs::read_to_string(root.join("etc/pacman.conf")).map_err(|e| err(e.to_string()))?;
    let mut output = String::new();
    let mut options = false;
    let mut found = false;
    for line in original.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            options = trimmed == "[options]";
            if options {
                found = true;
                output.push_str("[options]\nLocalFileSigLevel = Optional\n");
                continue;
            }
        }
        if options
            && trimmed.split_once('=').is_some_and(|(key, _)| key.trim() == "LocalFileSigLevel")
        {
            continue;
        }
        output.push_str(line);
        output.push('\n');
    }
    if !found {
        return Err(err("The native pacman options are missing."));
    }
    let relative = "/var/lib/borea/aur-pacman.conf";
    let path = root.join(relative.trim_start_matches('/'));
    fs::create_dir_all(path.parent().ok_or_else(|| err("No AUR state directory."))?)
        .map_err(|e| err(e.to_string()))?;
    crate::postinstall::persist(&path, output.as_bytes(), 0o600)?;
    Ok(relative.into())
}

pub(crate) fn install(
    root: &Path,
    archives: &Path,
    recipes: &[AurRecipe],
    mut run: impl FnMut(&str, Vec<String>) -> Result<String, Hint>,
    mut completed: impl FnMut(&AurRecipe) -> Result<(), Hint>,
) -> Result<(), Hint> {
    if recipes.len() > 128 || !recipes.iter().all(valid_recipe) {
        return Err(err("The reviewed AUR recipe list is invalid."));
    }
    // Verify every approved archive before creating an account or running code.
    for recipe in recipes {
        let path = archives.join(format!("{}.tar.gz", recipe.base));
        let metadata = fs::symlink_metadata(&path).map_err(|e| err(e.to_string()))?;
        if !metadata.is_file() || metadata.len() > 64 * 1024 * 1024 {
            return Err(err("The pinned AUR archive must be a bounded regular file."));
        }
        let data = fs::read(&path).map_err(|e| err(e.to_string()))?;
        if !format!("{:x}", Sha256::digest(data)).eq_ignore_ascii_case(&recipe.sha256) {
            return Err(err("The AUR recipe changed after Review."));
        }
    }
    let passwd = fs::read_to_string(root.join("etc/passwd")).map_err(|e| err(e.to_string()))?;
    let account = passwd.lines().find(|line| line.starts_with("borea-build:"));
    if let Some(account) = account {
        let fields = account.split(':').collect::<Vec<_>>();
        let system_id = |s: Option<&&str>| {
            s.and_then(|v| v.parse::<u32>().ok()).is_some_and(|v| v > 0 && v < 1000)
        };
        if !system_id(fields.get(2))
            || !system_id(fields.get(3))
            || fields.get(5) != Some(&"/var/lib/borea-build")
        {
            return Err(err("The AUR build account is not Borea's dedicated system account."));
        }
    } else if recipes.is_empty() {
        return Ok(());
    }
    if !recipes.is_empty() {
        run(
            "/usr/bin/pacman",
            args(&["-S", "--needed", "--noconfirm", "--", "base-devel", "git"]),
        )?;
        if account.is_none() {
            run(
                "/usr/bin/useradd",
                args(&[
                    "--system",
                    "--user-group",
                    "--create-home",
                    "--home-dir",
                    "/var/lib/borea-build",
                    "--shell",
                    "/usr/bin/nologin",
                    "borea-build",
                ]),
            )?;
        }
    }
    let local_config = if recipes.is_empty() { None } else { Some(local_package_config(root)?) };
    for recipe in recipes {
        if !recipe.dependencies.is_empty() {
            run(
                "/usr/bin/pacman",
                args(&["-S", "--needed", "--noconfirm", "--"])
                    .into_iter()
                    .chain(recipe.dependencies.iter().cloned())
                    .collect(),
            )?;
        }
        let directory = format!("/var/lib/borea-build/{}", recipe.base);
        let destination = root.join(directory.trim_start_matches('/'));
        // Clean a failed attempt without following links from the unprivileged build.
        if let Ok(metadata) = fs::symlink_metadata(&destination) {
            if !metadata.is_dir() {
                return Err(err("The AUR build directory is not a directory."));
            }
            fs::remove_dir_all(&destination).map_err(|e| err(e.to_string()))?;
        }
        fs::create_dir_all(&destination).map_err(|e| err(e.to_string()))?;
        fs::copy(
            archives.join(format!("{}.tar.gz", recipe.base)),
            destination.join("recipe.tar.gz"),
        )
        .map_err(|e| err(e.to_string()))?;
        run(
            "/usr/bin/chown",
            vec!["-R".into(), "borea-build:borea-build".into(), directory.clone()],
        )?;
        run(
            "/usr/sbin/runuser",
            args(&["-u", "borea-build", "--", "/usr/bin/tar", "-xzf"])
                .into_iter()
                .chain([
                    format!("{directory}/recipe.tar.gz"),
                    "--strip-components=1".into(),
                    "-C".into(),
                    directory.clone(),
                    "--no-same-owner".into(),
                ])
                .collect(),
        )?;
        // Evaluate build metadata only as the unprivileged builder. Import only
        // complete fingerprints declared by this pinned recipe, never skip PGP.
        let metadata = run(
            "/usr/sbin/runuser",
            args(&["-u", "borea-build", "--", "/usr/bin/env"])
                .into_iter()
                .chain([
                    format!("--chdir={directory}"),
                    "/usr/bin/makepkg".into(),
                    "--printsrcinfo".into(),
                ])
                .collect(),
        )?;
        for key in signing_keys(&metadata)? {
            run(
                "/usr/sbin/runuser",
                args(&[
                    "-u",
                    "borea-build",
                    "--",
                    "/usr/bin/gpg",
                    "--batch",
                    "--keyserver",
                    "hkps://keyserver.ubuntu.com",
                    "--recv-keys",
                ])
                .into_iter()
                .chain([key])
                .collect(),
            )?;
        }
        // No sudo, --syncdeps or integrity/signature bypass is given to makepkg.
        run(
            "/usr/sbin/runuser",
            args(&["-u", "borea-build", "--", "/usr/bin/env"])
                .into_iter()
                .chain([
                    format!("--chdir={directory}"),
                    "/usr/bin/makepkg".into(),
                    "--cleanbuild".into(),
                    "--noconfirm".into(),
                ])
                .collect(),
        )?;
        let mut artifacts = Vec::new();
        let mut names = std::collections::BTreeSet::new();
        for entry in fs::read_dir(&destination).map_err(|e| err(e.to_string()))? {
            let entry = entry.map_err(|e| err(e.to_string()))?;
            let name = entry.file_name().to_string_lossy().to_string();
            if name.ends_with(".pkg.tar.zst") || name.ends_with(".pkg.tar.xz") {
                if !entry.file_type().map_err(|e| err(e.to_string()))?.is_file() {
                    return Err(err("A built AUR artifact is not a regular file."));
                }
                let artifact = format!("{directory}/{name}");
                let metadata = run(
                    "/usr/bin/pacman",
                    vec![
                        "-Qp".into(),
                        "--config".into(),
                        local_config.clone().ok_or_else(|| err("No local AUR policy."))?,
                        artifact.clone(),
                    ],
                )?;
                if let Some(name) = metadata
                    .split_whitespace()
                    .next()
                    .filter(|n| recipe.packages.iter().any(|p| p == n))
                {
                    names.insert(name.to_owned());
                    artifacts.push(artifact);
                }
            }
        }
        if !recipe.packages.iter().all(|p| names.contains(p)) {
            return Err(err("The AUR build did not produce all reviewed packages."));
        }
        artifacts.sort();
        run(
            "/usr/bin/pacman",
            vec![
                "-U".to_owned(),
                "--needed".into(),
                "--noconfirm".into(),
                "--config".into(),
                local_config.clone().ok_or_else(|| err("No local AUR policy."))?,
                "--".into(),
            ]
            .into_iter()
            .chain(artifacts)
            .collect(),
        )?;
        completed(recipe)?;
    }
    if root.join("var/lib/borea-build/.gnupg").is_dir() {
        let _ = run(
            "/usr/sbin/runuser",
            args(&[
                "-u",
                "borea-build",
                "--",
                "/usr/bin/gpgconf",
                "--homedir",
                "/var/lib/borea-build/.gnupg",
                "--kill",
                "all",
            ]),
        );
    }
    run("/usr/bin/userdel", args(&["--remove", "borea-build"]))?;
    let policy = root.join("var/lib/borea/aur-pacman.conf");
    if policy.exists() {
        fs::remove_file(policy).map_err(|e| err(e.to_string()))?;
    }
    Ok(())
}

fn signing_keys(metadata: &str) -> Result<std::collections::BTreeSet<String>, Hint> {
    metadata
        .lines()
        .filter_map(|line| line.trim().strip_prefix("validpgpkeys = "))
        .map(|key| {
            if ![40, 64].contains(&key.len()) || !key.bytes().all(|b| b.is_ascii_hexdigit()) {
                Err(err("An AUR signing key must be a complete fingerprint."))
            } else {
                Ok(key.to_ascii_uppercase())
            }
        })
        .collect()
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    struct Fixture {
        root: std::path::PathBuf,
        archives: std::path::PathBuf,
        recipe: AurRecipe,
    }
    impl Fixture {
        fn new() -> Self {
            let root = std::env::temp_dir().join(format!(
                "borea-aur-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir_all(root.join("etc")).unwrap();
            fs::write(root.join("etc/passwd"), "root:x:0:0:root:/root:/bin/bash\n").unwrap();
            fs::write(root.join("etc/pacman.conf"), "[options]\nSigLevel = Required DatabaseOptional\nLocalFileSigLevel = Required\n[core]\nServer = https://example.org/$repo\n").unwrap();
            let archives = root.join("archives");
            fs::create_dir_all(&archives).unwrap();
            fs::write(archives.join("example-bin.tar.gz"), b"reviewed recipe").unwrap();
            Self {
                root,
                archives,
                recipe: AurRecipe {
                    base: "example-bin".into(),
                    url: "https://aur.archlinux.org/cgit/aur.git/snapshot/example-bin.tar.gz"
                        .into(),
                    sha256: format!("{:x}", Sha256::digest(b"reviewed recipe")),
                    packages: vec!["example-bin".into()],
                    dependencies: vec!["gtk3".into()],
                },
            }
        }
        fn run(&self, program: &str, args: &[String]) -> Result<String, Hint> {
            if args.last().is_some_and(|s| s == "--noconfirm")
                && args.iter().any(|s| s == "/usr/bin/makepkg")
            {
                fs::write(
                    self.root.join("var/lib/borea-build/example-bin/example.pkg.tar.zst"),
                    b"package",
                )
                .unwrap();
            }
            Ok(if program == "/usr/bin/pacman" && args[0] == "-Qp" {
                let policy =
                    fs::read_to_string(self.root.join("var/lib/borea/aur-pacman.conf")).unwrap();
                assert!(policy.contains("LocalFileSigLevel = Optional"));
                assert!(policy.contains("SigLevel = Required DatabaseOptional"));
                assert!(policy.contains("Server = https://example.org/$repo"));
                "example-bin 1.0-1\n".into()
            } else {
                String::new()
            })
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }
    #[test]
    fn pinned_build_is_unprivileged_and_checkpoint_follows_package_install() {
        let f = Fixture::new();
        let mut calls = vec![];
        let mut done = vec![];
        install(
            &f.root,
            &f.archives,
            std::slice::from_ref(&f.recipe),
            |p, a| {
                calls.push((p.to_owned(), a.clone()));
                f.run(p, &a)
            },
            |r| {
                done.push(r.base.clone());
                Ok(())
            },
        )
        .unwrap();
        let build = calls
            .iter()
            .position(|(_, a)| {
                a.iter().any(|v| v == "/usr/bin/makepkg")
                    && a.last().is_some_and(|s| s == "--noconfirm")
            })
            .unwrap();
        assert_eq!(calls[build].0, "/usr/sbin/runuser");
        assert_eq!(&calls[build].1[..3], ["-u", "borea-build", "--"]);
        assert!(!calls.iter().any(|(p, a)| p == "/usr/bin/makepkg"
            || a.iter().any(|v| v == "--skippgpcheck" || v == "--skipinteg" || v == "--syncdeps")));
        let installed =
            calls.iter().position(|(p, a)| p == "/usr/bin/pacman" && a[0] == "-U").unwrap();
        assert!(installed > build);
        assert_eq!(done, ["example-bin"]);
        let original = fs::read_to_string(f.root.join("etc/pacman.conf")).unwrap();
        assert!(original.contains("LocalFileSigLevel = Required"));
        assert!(original.contains("SigLevel = Required DatabaseOptional"));
        assert!(!f.root.join("var/lib/borea/aur-pacman.conf").exists());
        for (_, argv) in calls.iter().filter(|(p, _)| p == "/usr/bin/pacman") {
            assert_eq!(argv.iter().any(|a| a == "--config"), argv[0] == "-U" || argv[0] == "-Qp");
        }
    }
    #[test]
    fn changed_archives_fail_before_any_command_or_checkpoint() {
        let f = Fixture::new();
        fs::write(f.archives.join("example-bin.tar.gz"), b"changed").unwrap();
        let called = std::cell::Cell::new(false);
        assert!(
            install(
                &f.root,
                &f.archives,
                std::slice::from_ref(&f.recipe),
                |_, _| {
                    called.set(true);
                    Ok(String::new())
                },
                |_| {
                    called.set(true);
                    Ok(())
                }
            )
            .is_err()
        );
        assert!(!called.get());
    }
    #[test]
    fn a_failed_build_never_installs_or_checkpoints_a_package() {
        let f = Fixture::new();
        let mut installed = false;
        let mut done = false;
        assert!(
            install(
                &f.root,
                &f.archives,
                std::slice::from_ref(&f.recipe),
                |p, a| {
                    installed |= p == "/usr/bin/pacman" && a[0] == "-U";
                    if a.last().is_some_and(|v| v == "--noconfirm")
                        && a.iter().any(|v| v == "/usr/bin/makepkg")
                    {
                        Err(err("build failed"))
                    } else {
                        Ok(String::new())
                    }
                },
                |_| {
                    done = true;
                    Ok(())
                }
            )
            .is_err()
        );
        assert!(!installed && !done);
    }
    #[test]
    fn retry_reuses_only_the_dedicated_system_account() {
        let f = Fixture::new();
        fs::write(
            f.root.join("etc/passwd"),
            "borea-build:x:222:222::/var/lib/borea-build:/usr/bin/nologin\n",
        )
        .unwrap();
        let mut created = false;
        install(
            &f.root,
            &f.archives,
            std::slice::from_ref(&f.recipe),
            |p, a| {
                created |= p == "/usr/bin/useradd";
                f.run(p, &a)
            },
            |_| Ok(()),
        )
        .unwrap();
        assert!(!created);
        fs::write(f.root.join("etc/passwd"), "borea-build:x:1000:1000::/home/person:/bin/bash\n")
            .unwrap();
        assert!(install(&f.root, &f.archives, &[], |_, _| Ok(String::new()), |_| Ok(())).is_err());
    }
    #[test]
    fn signing_keys_must_be_complete_fingerprints() {
        let key = "0123456789ABCDEF0123456789ABCDEF01234567";
        assert!(signing_keys(&format!("\tvalidpgpkeys = {key}\n")).unwrap().contains(key));
        for key in ["ABCDEF12", "--evil", "0123456789ABCDEF0123456789ABCDEF0123456Z"] {
            assert!(signing_keys(&format!("validpgpkeys = {key}")).is_err());
        }
    }
}
