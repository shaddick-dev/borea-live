//! Terminal presentation shared by every command: sections, aligned rows, lists.

use std::fmt::Display;
use std::io::{self, Write};

pub type Out<'a> = io::StdoutLock<'a>;

pub const GIB: f64 = (1u64 << 30) as f64;

pub fn section(out: &mut Out, title: &str) -> io::Result<()> {
    writeln!(out, "\n{title}\n{}", "─".repeat(title.chars().count()))
}

/// One aligned line: a short label column, then the value.
pub fn row(out: &mut Out, label: &str, value: impl Display) -> io::Result<()> {
    writeln!(out, "  {label:<13} {value}")
}

pub fn list(items: impl IntoIterator<Item = impl Display>) -> String {
    items.into_iter().map(|item| item.to_string()).collect::<Vec<_>>().join(", ")
}
