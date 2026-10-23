//! Install the helper and its compatibility command without invoking Cargo at launch.
use std::fs;
use std::io;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

pub fn commands(source: &Path, directory: &Path) -> io::Result<()> {
    fs::create_dir_all(directory)?;
    for name in ["borea-helper", "borea-flash"] {
        let destination = directory.join(name);
        if source == destination {
            continue;
        }
        let temporary = directory.join(format!(".{name}.{}.new", std::process::id()));
        fs::copy(source, &temporary)?;
        fs::set_permissions(&temporary, fs::Permissions::from_mode(0o755))?;
        fs::rename(temporary, destination)?;
    }
    Ok(())
}

pub fn desktop_entry(helper: &Path) -> String {
    // Desktop-entry values unescape backslashes before Exec parses its quoted
    // arguments. Percent signs must also be escaped as literal field codes.
    let executable = helper
        .to_string_lossy()
        .replace('\\', "\\\\\\\\")
        .replace('"', "\\\\\"")
        .replace('`', "\\\\`")
        .replace('$', "\\\\$")
        .replace('%', "%%");
    format!(
        "[Desktop Entry]\nType=Application\nName=Borea TUI Helper\n\
         Comment=Prepare the Borea Live ISO and write it to a USB drive\n\
         Exec=\"{executable}\"\nTerminal=true\nIcon=drive-removable-media-usb\n\
         Categories=System;\nKeywords=Borea;ISO;USB;flash;installer;\n"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn installed_commands_work_after_build_output_is_removed() {
        let directory = crate::image::tests::temp_dir("install");
        let source = directory.join("source");
        fs::write(&source, b"binary").unwrap();
        let bin = directory.join("bin");
        commands(&source, &bin).unwrap();
        fs::remove_file(source).unwrap();
        for name in ["borea-helper", "borea-flash"] {
            assert_eq!(fs::read(bin.join(name)).unwrap(), b"binary");
            assert_eq!(fs::metadata(bin.join(name)).unwrap().permissions().mode() & 0o777, 0o755);
        }
        commands(&bin.join("borea-helper"), &bin).unwrap();
        assert_eq!(fs::read(bin.join("borea-flash")).unwrap(), b"binary");
    }

    #[test]
    fn launcher_runs_helper_directly_and_quotes_its_path() {
        let entry = desktop_entry(Path::new("/home/Example User/100%/borea-helper"));
        assert!(entry.contains("Name=Borea TUI Helper\n"));
        assert!(entry.contains("Exec=\"/home/Example User/100%%/borea-helper\"\n"));
        assert!(entry.contains("Terminal=true\n"));
        assert!(!entry.contains("cargo"));
    }
}
