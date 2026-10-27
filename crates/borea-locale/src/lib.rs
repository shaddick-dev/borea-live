#![forbid(unsafe_code)]
use borea_core::{Hint, LocaleChoice, hint};
use serde::Serialize;
#[derive(Debug, Clone, Copy, Serialize)]
pub struct Locale {
    pub id: &'static str,
    pub english: &'static str,
    pub native: &'static str,
}
#[derive(Debug, Clone, Copy, Serialize)]
pub struct KeyboardEntry {
    pub layout: &'static str,
    pub variant: &'static str,
    pub description: &'static str,
}
#[derive(Debug, Clone, Copy, Serialize)]
pub struct CountryDefault {
    pub country: &'static str,
    pub locale: &'static str,
    pub layout: &'static str,
    pub console: &'static str,
    pub timezone: &'static str,
}
include!("generated.rs");
pub fn suggest(country: &str) -> Option<&'static CountryDefault> {
    DEFAULTS.iter().find(|d| d.country.eq_ignore_ascii_case(country))
}
pub fn valid_console(key: &str) -> bool {
    CONSOLE_MAP
        .lines()
        .filter(|l| !l.starts_with('#'))
        .any(|l| l.split_whitespace().next() == Some(key))
}
const CONSOLE_MAP: &str = include_str!("../data/kbd-model-map");
pub fn validate(choice: &LocaleChoice) -> Result<(), Hint> {
    if !LOCALES.iter().any(|l| l.id == choice.lang) {
        return Err(hint("locale", "Select a UTF-8 system locale."));
    }
    if !KEYBOARDS
        .iter()
        .any(|k| k.layout == choice.keyboard.layout && k.variant == choice.keyboard.variant)
    {
        return Err(hint("keyboard", "Select a valid keyboard layout and variant."));
    }
    if !valid_console(&choice.keyboard.console) {
        return Err(hint("console", "Select a known console keymap."));
    }
    if !TIMEZONES.contains(&choice.timezone.as_str()) {
        return Err(hint("timezone", "Select a known time zone."));
    }
    Ok(())
}
#[derive(Debug, Clone, Serialize)]
pub struct Suggestions {
    pub locale: String,
    pub layout: String,
    pub variant: String,
    pub console: String,
    pub timezone: String,
    pub keyboard_touched: bool,
    pub timezone_touched: bool,
}
impl Suggestions {
    pub fn change_language(&mut self, locale: &str) {
        self.locale = locale.into();
        if let Some(d) = DEFAULTS.iter().find(|d| d.locale == locale) {
            if !self.keyboard_touched {
                self.layout = d.layout.into();
                self.variant.clear();
                self.console = d.console.into();
            }
            if !self.timezone_touched {
                self.timezone = d.timezone.into();
            }
        }
    }
}
