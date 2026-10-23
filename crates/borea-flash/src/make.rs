//! The build stage of `borea-flash --make`: rebuild Borea Live when its sources
//! changed since the last image, showing Buildroot's progress, then hand over to
//! writing the stick.

use std::collections::{BTreeSet, VecDeque};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

use ratatui::Frame;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Padding, Paragraph, Wrap};

const ACCENT: Color = Color::Rgb(0xa2, 0xc4, 0xa8);
const SUB: Color = Color::Rgb(0x8a, 0x96, 0x8c);
const WARN: Color = Color::Rgb(0xe3, 0x9b, 0x84);

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
    let sources =
        [repo.join("crates"), repo.join("live"), repo.join("Cargo.lock"), repo.join("Cargo.toml")];
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
            started: Instant::now(),
            current: None,
            finished: BTreeSet::new(),
            tail: VecDeque::new(),
            stop_asked: false,
            failed: None,
            log,
        }
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

    pub fn render(&self, frame: &mut Frame, area: Rect) {
        let area = area.inner(ratatui::layout::Margin { horizontal: 2, vertical: 0 });
        let [_, title, status, _, output, ask] = Layout::vertical([
            Constraint::Length(1),
            Constraint::Length(2),
            Constraint::Length(2),
            Constraint::Length(1),
            Constraint::Fill(1),
            Constraint::Length(3),
        ])
        .areas(area);

        let elapsed = clock(self.started.elapsed());
        let heading = match &self.failed {
            Some(_) => Span::styled("✕ The build stopped.", Style::new().fg(WARN).bold()),
            None => Span::raw("Building Borea Live").bold(),
        };
        frame.render_widget(
            Paragraph::new(Line::from(vec![heading, Span::styled(format!("   {elapsed}"), SUB)])),
            title,
        );

        let status_line = match (&self.failed, &self.current) {
            (Some(why), _) => Line::from(Span::styled(why.clone(), WARN)),
            (None, Some(current)) => Line::from(vec![
                Span::styled("● ", ACCENT),
                Span::raw(current.clone()),
                Span::styled(format!("   {} packages done", self.finished.len()), SUB),
            ]),
            (None, None) => Line::from(Span::styled("Preparing…", SUB)),
        };
        let note = Line::from(Span::styled(
            "The first build compiles everything from source and takes a few hours. Later ones reuse what is unchanged.",
            SUB,
        ));
        frame.render_widget(
            Paragraph::new(vec![status_line, note]).wrap(Wrap { trim: true }),
            status,
        );

        let lines: Vec<Line> =
            self.tail.iter().map(|l| Line::from(Span::styled(l.clone(), SUB))).collect();
        let block = Block::new()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::new().fg(Color::Rgb(0x39, 0x44, 0x3b)))
            .title(Span::styled(format!(" Build output · full log: {} ", self.log.display()), SUB))
            .padding(Padding::horizontal(1));
        frame.render_widget(Paragraph::new(lines).block(block), output);

        let footer = match (&self.failed, self.stop_asked) {
            (Some(_), _) => vec![Line::from(Span::styled(
                "r try again (picks up where it stopped) · q quit",
                SUB,
            ))],
            (None, true) => vec![
                Line::from(Span::styled(
                    "Stop the build? Everything finished so far is kept for next time.",
                    Style::new().fg(WARN).add_modifier(Modifier::BOLD),
                )),
                Line::from(Span::styled("y stop · any other key keep building", SUB)),
            ],
            (None, false) => vec![Line::from(Span::styled(
                "When it finishes, Borea moves straight on to writing your USB stick · Esc stop",
                SUB,
            ))],
        };
        frame.render_widget(Paragraph::new(footer), ask);
    }
}

fn clock(elapsed: Duration) -> String {
    let s = elapsed.as_secs();
    format!("{}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::crossterm::event::{KeyEventKind, KeyEventState};

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
        terminal.draw(|f| view.render(f, f.area())).unwrap();
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
        assert!(text.contains("1 packages done"));
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
        assert!(screen(&view).contains("The build stopped."));
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
