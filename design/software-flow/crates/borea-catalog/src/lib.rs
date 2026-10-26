#![forbid(unsafe_code)]
//! # borea-catalog
//!
//! The model behind Borea's software page (step 05).
//!
//! * Traditional install groups ([`groups`]): one base environment plus any add-ons, written once
//!   with distro-neutral package keys and mapped onto each distro's real package names.
//! * One app list for every distro ([`catalog::APPS`]), grouped into categories.
//! * Sources are **only the chosen distro's repos**, the AUR when that distro is Arch, and Flathub
//!   ([`source::Sources`]). Another distro's repos can never be switched on.
//! * One "install from" preference ([`resolve::Prefer`]): distro packages first, or Flathub first.
//! * Every ticked app resolves to exactly one source; ticking an app whose only source is off
//!   switches that source on ([`resolve::Selection::toggle`]).
//! * [`resolve::plan`] groups the result into batches per source for the installer to run.
//! * Drivers are automatic: [`hardware::scan`] reads the machine, [`drivers::plan_drivers`] picks
//!   graphics (NVIDIA by generation, AMD, Intel), hybrid/MUX laptop setup, microcode and firmware.

pub mod catalog;
pub mod drivers;
pub mod groups;
pub mod hardware;
pub mod probe;
pub mod resolve;
pub mod source;

pub use catalog::{APPS, App, Category, PRESETS, SEARCH_ONLY, app, listed};
pub use drivers::{DriverPlan, Found, NvDriver, Pkg, nvidia_driver, plan_drivers};
pub use groups::{
    DESKTOPS, FIREWALL, GROUPS, Group, GroupError, GroupPlan, Kind, PKGS, Picks, REQUIRED,
    SECTIONS, addon_for_app, group, package, plan_groups, plan_groups_with,
};
pub use hardware::{Chassis, Cpu, Family, Gpu, Hardware, Mux, MuxMode, NvGen, scan};
pub use resolve::{Pick, Plan, Prefer, Resolution, Selection, candidates, plan, resolve};
pub use source::{Distro, REPOS, Repo, Source, SourceError, Sources, Trust, repos_for};
