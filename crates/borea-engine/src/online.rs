//! Four rolling targets, one neutral live environment. These are provisioning
//! contracts, not claims that the corresponding execution adapter is complete.

use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Target {
    Arch,
    Fedora,
    Debian,
    Opensuse,
}

#[derive(Debug, Serialize)]
pub struct Profile {
    pub target: Target,
    pub channel: &'static str,
    pub development_channel: bool,
    pub repository: &'static str,
    pub package_manager: &'static str,
    pub bootstrap: &'static str,
    pub verification: &'static str,
    pub kernel_package: &'static str,
    pub administrator_group: &'static str,
    pub initramfs: &'static str,
    pub execution_available: bool,
}

impl Target {
    pub const ALL: [Self; 4] = [Self::Arch, Self::Fedora, Self::Debian, Self::Opensuse];

    pub fn parse(id: &str) -> Result<Self, String> {
        match id {
            "arch" => Ok(Self::Arch),
            "fedora" => Ok(Self::Fedora),
            "debian" => Ok(Self::Debian),
            "opensuse" | "tumbleweed" => Ok(Self::Opensuse),
            _ => Err(format!("unknown target `{id}`: choose arch, fedora, debian or opensuse")),
        }
    }

    pub fn profile(self) -> Profile {
        let (
            channel,
            development_channel,
            repository,
            manager,
            bootstrap,
            verification,
            kernel,
            group,
            initramfs,
        ) = match self {
            Self::Arch => (
                "rolling",
                false,
                "https://geo.mirror.pkgbuild.com",
                "pacman",
                "signed Arch bootstrap archive",
                "pinned release key and signed packages",
                "linux",
                "wheel",
                "mkinitcpio",
            ),
            Self::Fedora => (
                "rawhide",
                true,
                "https://dl.fedoraproject.org/pub/fedora/linux/development/rawhide/Everything/x86_64/os/",
                "dnf",
                "verified Fedora tool root",
                "Fedora signing keys and signed RPMs",
                "kernel",
                "wheel",
                "dracut",
            ),
            Self::Debian => (
                "sid",
                true,
                "https://deb.debian.org/debian",
                "apt",
                "verified Debian tool root",
                "Debian archive keyring, signed InRelease and package hashes",
                "linux-image-amd64",
                "sudo",
                "update-initramfs",
            ),
            Self::Opensuse => (
                "tumbleweed",
                false,
                "https://download.opensuse.org/tumbleweed/repo/oss/",
                "zypper",
                "verified openSUSE tool root",
                "openSUSE signing keys, signed repository metadata and RPMs",
                "kernel-default",
                "wheel",
                "dracut",
            ),
        };
        Profile {
            target: self,
            channel,
            development_channel,
            repository,
            package_manager: manager,
            bootstrap,
            verification,
            kernel_package: kernel,
            administrator_group: group,
            initramfs,
            execution_available: self == Self::Arch,
        }
    }
}

/// The sequence adapters must implement. Downloads and verification precede
/// destructive operations; account and boot checks precede a success result.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Stage {
    Preflight,
    ResolveAndVerify,
    Review,
    ApproveDisk,
    RecheckDisk,
    Partition,
    Format,
    Mount,
    InstallPackages,
    ConfigureSystem,
    CreateAccount,
    ConfigureBoot,
    ValidateTarget,
    Unmount,
    Complete,
}

pub const STAGES: [Stage; 15] = [
    Stage::Preflight,
    Stage::ResolveAndVerify,
    Stage::Review,
    Stage::ApproveDisk,
    Stage::RecheckDisk,
    Stage::Partition,
    Stage::Format,
    Stage::Mount,
    Stage::InstallPackages,
    Stage::ConfigureSystem,
    Stage::CreateAccount,
    Stage::ConfigureBoot,
    Stage::ValidateTarget,
    Stage::Unmount,
    Stage::Complete,
];

/// Inspect the actual target contracts from a CLI without requiring a selected
/// disk or network access. No release artifacts are claimed to be resolved here.
pub fn profiles_json() -> String {
    serde_json::to_string_pretty(&Target::ALL.map(Target::profile))
        .expect("target profiles always serialize")
}

/// Progress cannot skip verification, approval, or final target checks. This
/// enforces ordering only; adapters are responsible for evidence for each step.
#[derive(Debug, Default)]
pub struct Progress {
    completed: usize,
}

impl Progress {
    pub fn next(&self) -> Option<Stage> {
        STAGES.get(self.completed).copied()
    }
    pub fn finish(&mut self, stage: Stage) -> Result<(), String> {
        if self.next() != Some(stage) {
            return Err(format!("expected {:?}, received {stage:?}", self.next()));
        }
        self.completed += 1;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn four_targets_keep_their_own_release_and_package_tools() {
        let profiles = Target::ALL.map(Target::profile);
        assert_eq!(
            profiles.each_ref().map(|p| p.channel),
            ["rolling", "rawhide", "sid", "tumbleweed"]
        );
        assert_eq!(
            profiles.each_ref().map(|p| p.package_manager),
            ["pacman", "dnf", "apt", "zypper"]
        );
        assert_eq!(
            profiles.each_ref().map(|p| p.administrator_group),
            ["wheel", "wheel", "sudo", "wheel"]
        );
        assert!(profiles.iter().all(|p| p.repository.starts_with("https://")));
        assert!(Target::parse("fedora-stable").is_err());
        assert_eq!(Target::parse("tumbleweed").unwrap(), Target::Opensuse);
    }

    #[test]
    fn cannot_skip_download_verification_or_disk_approval() {
        let mut progress = Progress::default();
        assert!(progress.finish(Stage::Partition).is_err());
        for stage in STAGES {
            assert_eq!(progress.next(), Some(stage));
            progress.finish(stage).unwrap();
            assert!(progress.finish(stage).is_err());
        }
        assert_eq!(progress.next(), None);
    }
}
