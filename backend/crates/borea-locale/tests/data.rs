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
