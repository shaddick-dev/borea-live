use borea_core::{Secret, Secrets};
use borea_exec::Redactor;
#[test]
fn secrets_never_appear_in_output_or_faults() {
    let s = Secrets {
        wifi: Some(Secret::new("wifi key".into())),
        passphrase: Secret::new("login password".into()),
        admin: Some(Secret::new("admin password".into())),
    };
    let r = Redactor::new(&s);
    let text = r.redact("program echoed login password, wifi key and admin password");
    assert!(!text.contains("login password"));
    assert!(!text.contains("wifi key"));
    assert!(!text.contains("admin password"));
    assert_eq!(text.matches("[REDACTED]").count(), 3);
}
