//! `borea-build concept-data`: write the catalog's compositors, shells, and extras into the
//! HTML concept (`design/windpad.html`), so the page and the Rust catalog agree.

use std::path::Path;

use borea_live_catalog::{Catalog, Source};
use serde_json::{Map, Value, json};

use crate::Result;

const START: &str = "// compose-data:start";
const END: &str = "// compose-data:end";

pub fn refresh(repo: &Path) -> Result {
    let page = repo.join("design/windpad.html");
    let html = std::fs::read_to_string(&page).map_err(|e| format!("{}: {e}", page.display()))?;
    let catalog = Catalog::bundled().map_err(|e| e.to_string())?;
    let updated = splice(&html, &data(&catalog))?;
    std::fs::write(&page, updated).map_err(|e| format!("{}: {e}", page.display()))?;
    println!("Updated {}", page.display());
    Ok(())
}

fn via(source: &Source) -> &'static str {
    match source {
        Source::Official { .. } => "official",
        Source::ThirdParty { .. } => "third-party",
        Source::Aur { .. } => "aur",
        Source::FirstLogin { .. } => "first-login",
    }
}

fn on<'a>(sources: impl Iterator<Item = (&'a borea_live_core::DistroId, &'a Source)>) -> Value {
    let map: Map<String, Value> = sources
        .map(|(distro, source)| {
            (distro.to_string(), json!({ "via": via(source), "label": source.to_string() }))
        })
        .collect();
    Value::Object(map)
}

/// The page's `desktops` object.
fn data(catalog: &Catalog) -> Value {
    let desktops = &catalog.desktops;
    json!({
        "compositors": desktops.compositors.iter().map(|c| json!({
            "id": c.id, "name": c.name, "style": c.style, "on": on(c.on.iter()),
        })).collect::<Vec<_>>(),
        "shells": desktops.shells.iter().map(|s| json!({
            "id": s.id, "name": s.name, "by": s.by, "framework": s.framework,
            "license": s.license, "summary": s.summary, "compositors": s.compositors,
            "madeFor": s.made_for, "on": on(s.on.iter()),
        })).collect::<Vec<_>>(),
        "extras": catalog.extras.extras.iter().map(|e| json!({
            "id": e.id, "name": e.name,
            "on": e.on.iter().map(|(distro, p)| (distro.to_string(), json!({
                "about": catalog.extras.on(distro).find(|o| o.extra.id == e.id).map(|o| o.describe()),
                "packages": p.packages, "default": p.default, "builtIn": p.built_in,
            }))).collect::<Map<String, Value>>(),
        })).collect::<Vec<_>>(),
    })
}

/// Replace the generated block in the page, between its two marker comments.
fn splice(html: &str, data: &Value) -> Result<String> {
    let start = html.find(START).ok_or("the page has no compose-data:start marker")?;
    let end = html[start..].find(END).ok_or("the page has no compose-data:end marker")? + start;
    let block = format!(
        "{START} generated from crates/borea-catalog/data/{{shells,extras}}.toml by `borea-build concept-data`\n    const desktops={data};\n    "
    );
    Ok(format!("{}{block}{}", &html[..start], &html[end..]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_page_gets_the_real_catalog() {
        let catalog = Catalog::bundled().unwrap();
        let value = data(&catalog);
        let mango =
            value["compositors"].as_array().unwrap().iter().find(|c| c["id"] == "mango").unwrap();
        assert_eq!(mango["on"]["debian"]["via"], "third-party");
        let butter =
            value["shells"].as_array().unwrap().iter().find(|s| s["id"] == "mangobutter").unwrap();
        assert_eq!(butter["madeFor"], json!(["debian"]));
        let gaming = &value["extras"][0];
        assert_eq!(gaming["name"], "Gaming");
        // Extras are opt-in on every distribution (data/extras.toml).
        assert_ne!(gaming["on"]["tumbleweed"]["default"], true);
        assert_eq!(gaming["on"]["bazzite"]["builtIn"], true);
        assert!(gaming["on"]["fedora"]["about"].as_str().unwrap().contains("RPM Fusion"));
    }

    #[test]
    fn only_the_generated_block_changes() {
        let html = "<script>\n    // compose-data:start old\n    const desktops={};\n    // compose-data:end\n    const state = 1;";
        let out = splice(html, &json!({"x": 1})).unwrap();
        assert!(out.contains(r#"const desktops={"x":1};"#));
        assert!(out.contains("// compose-data:end\n    const state = 1;"));
        assert!(!out.contains("desktops={};"));
    }
}
