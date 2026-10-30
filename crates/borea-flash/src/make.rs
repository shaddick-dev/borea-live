//! The build stage of `borea-flash --make`: rebuild Borea Live when its sources
//! changed since the last image, showing Buildroot's progress, then hand over to
//! writing the stick.

use std::collections::{BTreeSet, VecDeque};
use std::path::{Path, PathBuf};
use std::time::{Instant, SystemTime};

use ratatui::Frame;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::{Constraint, Layout};
use ratatui::style::{Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use crate::ui::{self, ACCENT, SUB, WARN};

/// Lines of build output kept on screen.
const TAIL: usize = 12;

/// The Borea checkout to build from: the one this binary runs from or the
/// current directory is in, else the one it was compiled in (for a copy
/// installed with `--install`), if that is still there.
pub fn repo() -> Option<PathBuf> {
    let starts = [std::env::current_exe().ok(), std::env::current_dir().ok()];
    let compiled = Path::new(env!("CARGO_MANIFEST_DIR")).to_path_buf();
    starts
        .into_iter()
        .flatten()
        .flat_map(|start| start.ancestors().map(Path::to_path_buf).collect::<Vec<_>>())
        .chain(compiled.ancestors().map(Path::to_path_buf))
        .find(|dir| dir.join("live/buildroot/external.desc").is_file())
}

/// The newest built image in the checkout.
pub fn newest_image(repo: &Path) -> Option<PathBuf> {
    newest(&repo.join("target/live/out"), &|p| {
        p.extension().is_some_and(|e| e == "iso")
            && p.file_name().is_some_and(|n| n.to_string_lossy().starts_with("borea-live-"))
    })
    .map(|(_, path)| path)
}

/// Whether Borea Live must be built: there is no image yet, or something it is
/// made from changed after the newest one.
pub fn needs_build(repo: &Path, image: Option<&Path>) -> bool {
    let Some(built) = image.and_then(|i| i.metadata().ok()?.modified().ok()) else { return true };
    // backend/dist holds the installer's service files and signing keys, which
    // go into the image; the rest of backend/ (docs, VM tests) does not.
    let sources = [
        repo.join("crates"),
        repo.join("live"),
        repo.join("backend/dist"),
        repo.join("Cargo.lock"),
        repo.join("Cargo.toml"),
    ];
    sources.iter().any(|source| {
        // Build output and documentation never change the image.
        newest(source, &|p| {
            !p.components().any(|c| c.as_os_str() == "target")
                && p.extension().is_none_or(|e| e != "md")
        })
        .is_some_and(|(changed, _)| changed > built)
    })
}

/// The most recently modified file under `root` (or `root` itself) that `keep` accepts.
fn newest(root: &Path, keep: &dyn Fn(&Path) -> bool) -> Option<(SystemTime, PathBuf)> {
    let meta = std::fs::symlink_metadata(root).ok()?;
    if meta.is_file() {
        return keep(root).then(|| Some((meta.modified().ok()?, root.to_path_buf()))).flatten();
    }
    if !meta.is_dir() {
        return None;
    }
    std::fs::read_dir(root)
        .ok()?
        .filter_map(|e| e.ok())
        .filter_map(|e| newest(&e.path(), keep))
        .max()
}

/// What the build stage asks the main loop to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuildRequest {
    Stop,
    Retry,
    Quit,
}

pub struct BuildView {
    compose: bool,
    started: Instant,
    /// The package Buildroot is working on.
    current: Option<String>,
    finished: BTreeSet<String>,
    tail: VecDeque<String>,
    stop_asked: bool,
    failed: Option<String>,
    log: PathBuf,
}

impl BuildView {
    pub fn new(log: PathBuf) -> Self {
        BuildView {
            compose: false,
            started: Instant::now(),
            current: None,
            finished: BTreeSet::new(),
            tail: VecDeque::new(),
            stop_asked: false,
            failed: None,
            log,
        }
    }

    pub fn composition(log: PathBuf) -> Self {
        let mut view = Self::new(log);
        view.compose = true;
        view
    }

    pub fn is_composing(&self) -> bool {
        self.compose
    }

    /// One line of build output.
    pub fn line(&mut self, line: &str) {
        // Buildroot announces each step as ">>> package version Step".
        if let Some(step) = line.strip_prefix(">>> ") {
            let mut words = step.split_whitespace();
            if let Some(package) = words.next() {
                let rest = step.splitn(3, ' ').nth(2).unwrap_or_default();
                if rest.starts_with("Installing") {
                    self.finished.insert(package.to_owned());
                }
                self.current = Some(format!("{package} · {}", rest.trim().to_lowercase()));
            }
        }
        if self.tail.len() == TAIL {
            self.tail.pop_front();
        }
        self.tail.push_back(line.chars().take(200).collect());
    }

    pub fn failed(&mut self, why: String) {
        self.failed = Some(why);
    }

    pub fn key(&mut self, key: KeyEvent) -> Option<BuildRequest> {
        let ctrl_c =
            key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c');
        if self.failed.is_some() {
            return match key.code {
                KeyCode::Char('r') | KeyCode::Enter => {
                    self.failed = None;
                    self.tail.clear();
                    self.started = Instant::now();
                    Some(BuildRequest::Retry)
                }
                KeyCode::Char('q') | KeyCode::Esc => Some(BuildRequest::Quit),
                _ if ctrl_c => Some(BuildRequest::Quit),
                _ => None,
            };
        }
        match (key.code, self.stop_asked) {
            (KeyCode::Char('y'), true) => Some(BuildRequest::Stop),
            (_, true) => {
                self.stop_asked = false;
                None
            }
            (KeyCode::Esc | KeyCode::Char('q'), false) => {
                self.stop_asked = true;
                None
            }
            _ if ctrl_c => {
                self.stop_asked = true;
                None
            }
            _ => None,
        }
    }

    pub fn render(&self, frame: &mut Frame) {
        let (note, keys): (Option<&str>, Vec<(&str, &str)>) = match (&self.failed, self.stop_asked)
        {
            (Some(_), _) => (None, vec![("r", "resume (keeps what was built)"), ("q", "quit")]),
            (None, true) => (
                Some("Stop the build? Everything finished so far is kept for next time."),
                vec![("y", "stop"), ("any other key", "keep building")],
            ),
            (None, false) => (None, vec![("Esc", "stop")]),
        };
        let area = ui::screen(frame, ui::Step::Image, note, &keys);
        let width = area.width;
        let elapsed =
            Span::styled(format!("   {}", ui::clock(self.started.elapsed().as_secs())), SUB);
        let mut lines = vec![Line::from("")];
        match &self.failed {
            Some(why) => {
                lines.push(Line::from(vec![
                    Span::styled("✕ The build stopped", Style::new().fg(WARN).bold()),
                    elapsed,
                ]));
                lines.push(Line::from(""));
                lines.extend(ui::text(why, width, Style::new().fg(WARN)));
            }
            None => {
                lines.push(Line::from(vec![Span::raw("Building Borea Live").bold(), elapsed]));
                lines.push(Line::from(""));
                lines.push(match &self.current {
                    Some(current) => {
                        Line::from(vec![Span::styled("● ", ACCENT), Span::raw(current.clone())])
                    }
                    None => Line::from(Span::styled("Preparing…", SUB)),
                });
                if !self.compose {
                    lines.push(Line::from(Span::styled(
                        format!("{} built", ui::count(self.finished.len(), "package", "packages")),
                        SUB,
                    )));
                }
                lines.push(Line::from(""));
                lines.extend(ui::text(
                    if self.compose {
                        "Assembling prebuilt Linux packages and Borea's Rust apps. No Borea release is needed. First use downloads the runtime and compiles the apps; later runs reuse both caches. When ready, choose your USB stick."
                    } else {
                        "The first build compiles a whole Linux system from source: an hour or more, depending on your machine. Later builds reuse what is unchanged. When it finishes, Borea moves straight on to your USB stick."
                    },
                    width,
                    Style::new().fg(SUB),
                ));
            }
        }
        lines.push(Line::from(""));
        let height = lines.len() as u16;
        let [top, output] =
            Layout::vertical([Constraint::Length(height), Constraint::Fill(1)]).areas(area);
        frame.render_widget(Paragraph::new(lines), top);

        let tail: Vec<Line> = self
            .tail
            .iter()
            .skip(self.tail.len().saturating_sub(output.height.saturating_sub(2) as usize))
            .map(|l| Line::from(Span::styled(l.clone(), SUB)))
            .collect();
        let mut log = match std::env::var_os("HOME") {
            Some(home) => match self.log.strip_prefix(&home) {
                Ok(rest) => format!("~/{}", rest.display()),
                Err(_) => self.log.display().to_string(),
            },
            None => self.log.display().to_string(),
        };
        // Too long for the box's bottom edge: the checkout-relative path will do.
        if log.chars().count() + 16 > width as usize {
            log = "target/live/build.log".into();
        }
        let block = ui::panel(" Build output ").title_bottom(
            Line::from(Span::styled(format!(" full log: {log} "), SUB)).right_aligned(),
        );
        if output.height >= 3 {
            frame.render_widget(Paragraph::new(tail).block(block), output);
        }
    }
}

/// What failed in the latest build, from its log: "package: error". With
/// packages building side by side, the log's last lines are usually another
/// package still finishing, so this looks for the first failed step instead.
pub fn failure(log: &str) -> Option<String> {
    let run = log.rsplit("── borea-flash --make ──").next().unwrap_or(log);
    let lines: Vec<&str> = run.lines().collect();
    let failed = lines
        .iter()
        .position(|l| l.starts_with("make[") && l.contains("]: *** ") && l.contains("/build/"))?;
    let package = lines[failed]
        .split("/build/")
        .nth(1)
        .and_then(|rest| rest.split('/').next())
        .unwrap_or("a package");
    let error = lines[failed.saturating_sub(60)..failed]
        .iter()
        .rev()
        .map(|l| l.trim())
        .find(|l| l.contains("error:") || l.contains("ERROR:") || l.starts_with("Error"))
        .map(|l| l.chars().take(240).collect::<String>());
    Some(match error {
        Some(error) => format!("{package} failed: {error}"),
        None => {
            format!("{package} failed; its error is just above the first `***` line in the log")
        }
    })
}

/// The end of a possibly very large log, at most `limit` bytes.
pub fn log_tail(path: &Path, limit: u64) -> Option<String> {
    use std::io::{Read, Seek, SeekFrom};
    let mut file = std::fs::File::open(path).ok()?;
    let length = file.metadata().ok()?.len();
    file.seek(SeekFrom::Start(length.saturating_sub(limit))).ok()?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes).ok()?;
    Some(String::from_utf8_lossy(&bytes).into_owned())
}

#[cfg(test)]
mod rebuild_tests {
    use super::needs_build;
    use std::time::{Duration, SystemTime};

    fn touch(path: &std::path::Path, age: Duration) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, b"x").unwrap();
        let when = SystemTime::now() - age;
        std::fs::File::options().write(true).open(path).unwrap().set_modified(when).unwrap();
    }

    #[test]
    fn installer_service_files_and_keys_count_but_backend_docs_do_not() {
        let repo = crate::image::tests::temp_dir("rebuild");
        let iso = repo.join("target/live/out/borea-live-0.1.0.iso");
        touch(&repo.join("crates/a.rs"), Duration::from_secs(300));
        touch(&iso, Duration::from_secs(200));
        assert!(!needs_build(&repo, Some(&iso)));
        touch(&repo.join("backend/docs/verification/run.txt"), Duration::from_secs(100));
        assert!(!needs_build(&repo, Some(&iso)), "verification logs don't change the image");
        touch(&repo.join("backend/dist/keys/arch.gpg"), Duration::from_secs(100));
        assert!(needs_build(&repo, Some(&iso)), "a new signing key does");
    }
}

#[cfg(test)]
mod failure_tests {
    use super::failure;

    #[test]
    fn names_the_package_that_failed_not_the_last_one_printed() {
        let log = "── borea-flash --make ──\n\
            make[1]: *** [package/pkg-generic.mk:263: /x/output/build/polkit-126/.stamp_configured] Error 1\n\
            ── borea-flash --make ──\n\
            checking for clang... no\n\
            configure: error: clang invalid, set CC_FOR_TARGET to a valid program\n\
            make[1]: *** [package/pkg-generic.mk:263: /x/output/build/host-limine-12.9.4/.stamp_configured] Error 1\n\
            make[1]: *** Waiting for unfinished jobs....\n\
            [100%] Built target spirv-reduce\n\
            make: *** [Makefile:83: _all] Error 2\n";
        assert_eq!(
            failure(log).as_deref(),
            Some(
                "host-limine-12.9.4 failed: configure: error: clang invalid, set CC_FOR_TARGET to a valid program"
            )
        );
        assert_eq!(failure("all good\n"), None);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::crossterm::event::{KeyEventKind, KeyEventState};
    use std::time::Duration;

    fn press(view: &mut BuildView, code: KeyCode) -> Option<BuildRequest> {
        view.key(KeyEvent {
            code,
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        })
    }

    fn screen(view: &BuildView) -> String {
        let mut terminal = Terminal::new(TestBackend::new(110, 26)).unwrap();
        terminal.draw(|f| view.render(f)).unwrap();
        let buffer = terminal.backend().buffer().clone();
        buffer
            .content
            .chunks(110)
            .map(|r| r.iter().map(|c| c.symbol()).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn follows_buildroot_steps() {
        let mut view = BuildView::new("/tmp/build.log".into());
        view.line(">>> host-gcc-final 15.3.0 Building");
        view.line("gcc -O2 -c foo.c");
        view.line(">>> host-gcc-final 15.3.0 Installing to host directory");
        view.line(">>> mesa3d 26.1.0 Configuring");
        let text = screen(&view);
        assert!(text.contains("mesa3d · configuring"), "{text}");
        assert!(text.contains("1 package built"));
        assert!(text.contains("gcc -O2 -c foo.c"));
    }

    #[test]
    fn stopping_asks_first_and_failure_offers_retry() {
        let mut view = BuildView::new("/tmp/build.log".into());
        assert_eq!(press(&mut view, KeyCode::Esc), None);
        assert!(screen(&view).contains("Stop the build?"));
        assert_eq!(press(&mut view, KeyCode::Char('n')), None);
        press(&mut view, KeyCode::Esc);
        assert_eq!(press(&mut view, KeyCode::Char('y')), Some(BuildRequest::Stop));

        view.failed("make failed (exit status: 2)".into());
        assert!(screen(&view).contains("The build stopped"));
        assert_eq!(press(&mut view, KeyCode::Char('r')), Some(BuildRequest::Retry));
    }

    #[test]
    fn rebuilds_only_when_sources_are_newer() {
        let dir = std::env::temp_dir().join(format!("borea-make-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("crates/x")).unwrap();
        std::fs::create_dir_all(dir.join("target/live/out")).unwrap();
        std::fs::write(dir.join("crates/x/lib.rs"), "old").unwrap();
        assert!(needs_build(&dir, None), "no image yet");

        std::thread::sleep(Duration::from_millis(20));
        let iso = dir.join("target/live/out/borea-live-0.1.0-20261022.iso");
        std::fs::write(&iso, "iso").unwrap();
        assert_eq!(newest_image(&dir).as_deref(), Some(iso.as_path()));
        assert!(!needs_build(&dir, Some(&iso)), "up to date");

        std::thread::sleep(Duration::from_millis(20));
        std::fs::write(dir.join("Cargo.toml"), "workspace changed").unwrap();
        assert!(needs_build(&dir, Some(&iso)), "workspace configuration changes the image");
        std::thread::sleep(Duration::from_millis(20));
        std::fs::write(&iso, "rebuilt iso").unwrap();

        std::thread::sleep(Duration::from_millis(20));
        std::fs::write(dir.join("crates/x/README.md"), "notes").unwrap();
        assert!(!needs_build(&dir, Some(&iso)), "documentation is not a source");

        std::thread::sleep(Duration::from_millis(20));
        std::fs::write(dir.join("crates/x/lib.rs"), "new").unwrap();
        assert!(needs_build(&dir, Some(&iso)), "a source changed");
        let _ = std::fs::remove_dir_all(dir);
    }
}
