//! `borea shells`: compose a desktop on an edition shipped without one.
//!
//! The path reads in one direction, narrowing at each step:
//! `borea shells` → `borea shells arch` → `borea shells arch niri`.

use std::io::Write;

use borea_catalog::{Catalog, Compositor, Distro, Offer, Source};

use crate::Result;
use crate::ui::{Out, list, section};

pub fn run(out: &mut Out, distro: Option<&&str>, compositor: Option<&&str>) -> Result {
    let catalog = Catalog::bundled()?;
    match (distro, compositor) {
        (None, _) => overview(out, &catalog),
        (Some(distro), None) => one_distro(out, &catalog, composable(&catalog, distro)?),
        (Some(distro), Some(compositor)) => {
            shells(out, &catalog, composable(&catalog, distro)?, compositor)
        }
    }
}

/// Resolve a distribution, explaining plainly when it cannot be composed.
fn composable<'a>(catalog: &'a Catalog, id: &str) -> Result<&'a Distro> {
    let offered =
        || list(catalog.distros.iter().filter(|d| d.composable().is_some()).map(|d| &d.id));
    match catalog.get(id) {
        None => Err(format!("no distribution `{id}`; composable ones are {}", offered()).into()),
        Some(distro) if distro.composable().is_none() => Err(format!(
            "{} ships its own desktop, so Borea leaves it untouched; \
             shells are offered on {}",
            distro.name,
            offered()
        )
        .into()),
        Some(distro) => Ok(distro),
    }
}

fn overview(out: &mut Out, catalog: &Catalog) -> Result {
    section(out, "Compose your desktop")?;
    writeln!(
        out,
        "  Editions without a preinstalled desktop let you build one in two steps:\n  \
         ① choose a compositor  ② choose a shell to live on it.\n"
    )?;
    for distro in catalog.distros.iter().filter(|d| d.composable().is_some()) {
        heading(out, distro)?;
        for offer in catalog.desktops.compositors(&distro.id) {
            compositor_line(out, distro, &offer)?;
            if offer.available() {
                let count = catalog.desktops.shells(&distro.id, &offer.item.id).count();
                let suggested = catalog.desktops.suggested(&distro.id, &offer.item.id);
                let suggested = suggested.map(|s| s.name.as_str()).unwrap_or("none");
                writeln!(out, "  {:<12} {count} shells · suggested {suggested}", "")?;
            }
        }
        extras(out, catalog, distro)?;
        writeln!(out)?;
    }
    writeln!(out, "  Next: borea shells <distro>   e.g. borea shells arch")?;
    writeln!(out, "  Or skip: every Minimal edition also installs with no desktop at all.")?;
    Ok(())
}

fn one_distro(out: &mut Out, catalog: &Catalog, distro: &Distro) -> Result {
    section(out, "Compose your desktop")?;
    heading(out, distro)?;
    for offer in catalog.desktops.compositors(&distro.id) {
        compositor_line(out, distro, &offer)?;
        if offer.available() {
            let names = catalog.desktops.shells(&distro.id, &offer.item.id).map(|s| &s.item.name);
            writeln!(out, "  {:<12} {}", "", list(names))?;
        }
    }
    writeln!(out, "  {:<12} install {} as shipped: a console login", "No desktop", distro.name)?;
    extras(out, catalog, distro)?;
    let first = catalog.desktops.compositors(&distro.id).find(|o| o.available());
    if let Some(first) = first {
        writeln!(out, "\n  Next: borea shells {} {}", distro.id, first.item.id)?;
    }
    Ok(())
}

fn shells(out: &mut Out, catalog: &Catalog, distro: &Distro, compositor: &str) -> Result {
    let Some(offer) = catalog.desktops.compositors(&distro.id).find(|o| o.item.id == compositor)
    else {
        let ids = catalog.desktops.compositors.iter().map(|c| &c.id);
        return Err(format!("no compositor `{compositor}`; choose from {}", list(ids)).into());
    };
    let Some(source) = offer.source else {
        return Err(format!("{} is not packaged for {}", offer.item.name, distro.name).into());
    };
    let edition = distro.composable().map(|e| e.name.as_str()).unwrap_or_default();
    section(out, &format!("{} · {edition} · {}", distro.name, offer.item.name))?;
    writeln!(out, "  ① {} — {}", offer.item.name, offer.item.style)?;
    writeln!(out, "     {} · {source}\n", source.badge())?;
    writeln!(out, "  ② Choose a shell\n")?;

    let suggested = catalog.desktops.suggested(&distro.id, compositor).map(|s| &s.id);
    for shell in catalog.desktops.ranked(&distro.id, compositor) {
        let (item, source) = (shell.item, shell.source.expect("shells() yields available"));
        let star = if Some(&item.id) == suggested { "★" } else { " " };
        writeln!(out, "  {star} {:<20} {} · {}", item.name, item.framework, item.license)?;
        writeln!(out, "    {:<20} {}", "", item.summary)?;
        writeln!(out, "    {:<20} {} · {source}\n", "", source.badge())?;
    }
    writeln!(out, "    {:<20} just {}, with its default config", "No shell", offer.item.name)?;
    writeln!(out, "    {:<20} bring your own bar and launcher later\n", "")?;
    extras(out, catalog, distro)?;
    writeln!(out)?;
    legend(out)?;
    Ok(())
}

fn heading(out: &mut Out, distro: &Distro) -> Result {
    let Some(edition) = distro.composable() else { return Ok(()) };
    writeln!(out, "  {} · {}", distro.name, edition.name)?;
    Ok(())
}

/// The extras checkboxes as they start out.
fn extras(out: &mut Out, catalog: &Catalog, distro: &Distro) -> Result {
    for (i, offered) in catalog.extras.on(&distro.id).enumerate() {
        let (tick, about) = match offered.provision {
            p if p.built_in => ("✓", "built in".to_owned()),
            p => (if p.default { "☑" } else { "☐" }, offered.describe()),
        };
        let label = if i == 0 { "Extras" } else { "" };
        writeln!(out, "  {label:<12} {tick} {}: {about}", offered.extra.name)?;
    }
    Ok(())
}

fn compositor_line(out: &mut Out, distro: &Distro, offer: &Offer<'_, Compositor>) -> Result {
    let c = offer.item;
    match offer.source {
        Some(source) => writeln!(out, "  {:<12} {:<40} {}", c.name, c.style, origin(source))?,
        None => writeln!(out, "  {:<12} not packaged for {}", c.name, distro.name)?,
    }
    Ok(())
}

/// Where a compositor comes from, in a few words.
fn origin(source: &Source) -> String {
    match source {
        Source::Official { suite: Some(suite), .. } => format!("official · {suite}"),
        Source::ThirdParty { repository, .. } => format!("third-party · {repository}"),
        other => other.badge().to_lowercase(),
    }
}

fn legend(out: &mut Out) -> Result {
    writeln!(
        out,
        "  ★ suggested: a setup made for this distribution, else the first official package.\n  \
         Third-party repositories are named in review before you approve.\n  \
         AUR builds and first-login setups start as you after first boot, never as root;\n  \
         a setup that installs packages asks for your password itself."
    )?;
    Ok(())
}
