#![allow(clippy::unwrap_used)]
use borea_locale::*;
#[test]
fn full_generated_tables() {
    assert!(LOCALES.len() > 300);
    assert!(KEYBOARDS.len() > 500);
    assert!(TIMEZONES.len() > 400);
    for (locale, layout) in
        [("en_US.UTF-8", "us"), ("en_GB.UTF-8", "gb"), ("fr_FR.UTF-8", "fr"), ("de_DE.UTF-8", "de")]
    {
        assert!(LOCALES.iter().any(|l| l.id == locale));
        let d = DEFAULTS.iter().find(|d| d.locale == locale).unwrap();
        assert_eq!(d.layout, layout);
        assert!(valid_console(d.console));
    }
}
#[test]
fn touched_keyboard_and_timezone_survive_language_change() {
    let mut s = Suggestions {
        locale: "en_GB.UTF-8".into(),
        layout: "gb".into(),
        variant: "".into(),
        console: "uk".into(),
        timezone: "Europe/London".into(),
        keyboard_touched: false,
        timezone_touched: false,
    };
    s.change_language("fr_FR.UTF-8");
    assert_eq!(s.layout, "fr");
    assert_eq!(s.timezone, "Europe/Paris");
    s.keyboard_touched = true;
    s.timezone_touched = true;
    s.change_language("en_US.UTF-8");
    assert_eq!(s.layout, "fr");
    assert_eq!(s.timezone, "Europe/Paris");
}

#[test]
fn every_country_suggestion_is_selectable() {
    for d in DEFAULTS {
        let choice = borea_core::LocaleChoice {
            lang: d.locale.into(),
            keyboard: borea_core::Keyboard {
                layout: d.layout.into(),
                variant: String::new(),
                console: d.console.into(),
            },
            timezone: d.timezone.into(),
        };
        assert!(validate(&choice).is_ok(), "Invalid country default: {d:?}");
    }
}

#[test]
fn host_specific_zone_pointers_are_not_installation_choices() {
    assert!(!TIMEZONES.contains(&"localtime"));
    assert!(!TIMEZONES.contains(&"posixrules"));
}
#[test]
fn a_connection_guess_is_a_valid_region() {
    let gb = borea_locale::guess("GB", Some("Europe/London")).unwrap();
    assert_eq!(gb.lang, "en_GB.UTF-8");
    assert_eq!(gb.keyboard.layout, "gb");
    assert_eq!(gb.keyboard.console, "uk");
    assert_eq!(gb.timezone, "Europe/London");
    // The connection's own time zone wins when it is a known one.
    assert_eq!(
        borea_locale::guess("us", Some("America/Chicago")).unwrap().timezone,
        "America/Chicago"
    );
    assert_eq!(borea_locale::guess("GB", Some("Mars/Base")).unwrap().timezone, "Europe/London");
    assert!(borea_locale::guess("ZZ", None).is_none());
    for d in borea_locale::DEFAULTS {
        assert!(borea_locale::validate(&borea_locale::guess(d.country, None).unwrap()).is_ok());
    }
}
