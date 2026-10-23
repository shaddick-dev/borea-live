//! Installation planning. Everything here is read-only with respect to disks:
//! it resolves the target's release, verifies its tools, checks the machine,
//! and produces an [`InstallPlan`] for the user to approve and the worker to
//! carry out.

pub mod arch;
pub mod disk;
pub mod fetch;
pub mod plan;

pub use plan::{InstallPlan, InstallRequest};
