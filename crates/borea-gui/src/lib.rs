//! Windpad and the Borea Live installer.
//!
//! Two faces of one workflow, sharing the catalog model, theme, widgets, wind, and
//! Wi-Fi: `windpad` is the landing pad; `borea-live` is the guided, full-screen
//! installer the live ISO boots into.
//!
//! Windpad:
//! The same landing pad for every target. It scans the machine, connects to Wi-Fi,
//! and guides the choice of family, distribution, edition, and, on editions that
//! ship without one, a desktop. It runs from Borea Live or any compatible live
//! session and never changes disks in this build.

pub mod app;
mod family_cards;
mod firmware;
pub mod icon;
pub mod live;
pub mod model;
pub mod theme;
pub mod widgets;
pub mod wifi;
pub mod wind;

pub use app::Windpad;
pub use live::LiveInstaller;
