//! Native account setup compiled by Rust for every target. Passwords are never
//! stored in these steps; the worker sends them to chpasswd on stdin only.

use crate::common::check_setting;
use crate::online::Target;

#[derive(Debug, PartialEq, Eq)]
pub struct AccountSetup {
    pub create_user: Vec<String>,
    pub lock_root: Vec<String>,
    pub sudoers: String,
}

pub fn setup(target: Target, username: &str) -> Result<AccountSetup, String> {
    check_setting("username", username)?;
    let group = target.profile().administrator_group;
    Ok(AccountSetup {
        create_user: [
            "useradd",
            "--create-home",
            "--user-group",
            "--groups",
            group,
            "--shell",
            "/bin/bash",
            username,
        ]
        .map(String::from)
        .into(),
        lock_root: ["passwd", "--lock", "root"].map(String::from).into(),
        sudoers: format!(
            "# Written by Borea: administrator accounts.\n%{group} ALL=(ALL:ALL) ALL\n"
        ),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_target_creates_a_normal_administrator_and_locks_root() {
        for target in Target::ALL {
            let setup = setup(target, "luke").unwrap();
            let group = target.profile().administrator_group;
            assert_eq!(setup.create_user.last().unwrap(), "luke");
            assert!(setup.create_user.windows(2).any(|w| w == ["--groups", group]));
            assert!(setup.create_user.contains(&"--create-home".into()));
            assert!(setup.sudoers.contains(&format!("%{group} ALL=(ALL:ALL) ALL")));
            assert_eq!(setup.lock_root, ["passwd", "--lock", "root"]);
        }
    }

    #[test]
    fn rejects_reserved_and_malformed_names_before_execution() {
        for user in ["root", "daemon", "--root", "a:b", "bad\nuser", ""] {
            for target in Target::ALL {
                assert!(setup(target, user).is_err());
            }
        }
    }
}
