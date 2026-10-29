//! The terminal interface: one screen at a time, keys in, requests out.
//!
//! [`App`] holds no devices and runs nothing. It turns keys into [`Request`]s that
//! the main loop carries out on worker threads, and turns their results back into
//! screens. That keeps every screen testable without touching a disk.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use borea_hardware::Disk;
use ratatui::Frame;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{List, ListItem, ListState, Paragraph};

use crate::image::Image;
use crate::target::{Target, Verdict};
use crate::ui::{self, ACCENT, SELECTED, SUB, Step, WARN, human};
use crate::write::{Outcome, Phase, Progress};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Checksum {
    /// No published checksum sits beside the image.
    Unpublished,
    Checking {
        done: u64,
    },
    Matches(PathBuf),
    Mismatch(PathBuf),
    Failed(String),
}

/// Whether this image can be rebuilt here, and whether it should be.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Checkout {
    /// Not running from (or built in) a Borea checkout: nothing to rebuild from.
    None,
    /// The image is newer than everything it is made from.
    Current,
    /// Something under crates/, live/ or Cargo.lock changed since the image.
    Changed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Screen {
    Choose,
    Confirm { typed: String },
    Working { progress: Progress, phase_started: Instant, stop_asked: bool },
    Finished(Result<Outcome, String>),
}

/// Something only the main loop can do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Request {
    Rescan,
    Unmount(Disk),
    Flash(String),
    Stop,
    Eject(String),
    /// Rebuild Borea Live from the checkout, then come back to writing.
    Rebuild,
}

pub struct App {
    pub image: Image,
    pub checksum: Checksum,
    pub targets: Vec<Target>,
    pub list: ListState,
    pub screen: Screen,
    pub message: Option<String>,
    pub quit: bool,
    pub checkout: Checkout,
}

impl App {
    pub fn new(image: Image, checksum: Checksum, targets: Vec<Target>) -> Self {
        let mut list = ListState::default();
        list.select((!targets.is_empty()).then_some(first_ready(&targets)));
        App {
            image,
            checksum,
            targets,
            list,
            screen: Screen::Choose,
            message: None,
            quit: false,
            checkout: Checkout::None,
        }
    }

    pub fn selected(&self) -> Option<&Target> {
        self.list.selected().and_then(|i| self.targets.get(i))
    }

    pub fn set_targets(&mut self, targets: Vec<Target>) {
        let keep = self.selected().map(|t| t.disk.name.clone());
        let index = keep.and_then(|name| targets.iter().position(|t| t.disk.name == name));
        self.list.select(index.or((!targets.is_empty()).then_some(first_ready(&targets))));
        self.targets = targets;
    }

    pub fn progress(&mut self, progress: Progress) {
        match &mut self.screen {
            Screen::Working { progress: current, phase_started, .. } => {
                if current.phase != progress.phase {
                    *phase_started = Instant::now();
                }
                *current = progress;
            }
            screen => {
                *screen =
                    Screen::Working { progress, phase_started: Instant::now(), stop_asked: false }
            }
        }
    }

    pub fn finished(&mut self, result: Result<Outcome, String>) {
        self.screen = Screen::Finished(result);
    }

    /// Handle one key press. Returns work for the main loop, if any.
    pub fn key(&mut self, key: KeyEvent) -> Option<Request> {
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            return self.interrupt();
        }
        self.message = None;
        match &mut self.screen {
            Screen::Choose => self.choose_key(key.code),
            Screen::Confirm { typed } => match key.code {
                KeyCode::Esc => {
                    self.screen = Screen::Choose;
                    None
                }
                KeyCode::Backspace => {
                    typed.pop();
                    None
                }
                KeyCode::Char(c) if typed.len() < 32 => {
                    typed.push(c);
                    None
                }
                KeyCode::Enter => {
                    let name = self.selected().map(|t| t.disk.name.clone()).unwrap_or_default();
                    let Screen::Confirm { typed } = &self.screen else { return None };
                    if *typed == name {
                        self.progress(Progress {
                            phase: Phase::Writing,
                            done: 0,
                            total: self.image.size,
                        });
                        Some(Request::Flash(name))
                    } else {
                        self.message =
                            Some(format!("Type {name} exactly to confirm, or Esc to go back."));
                        None
                    }
                }
                _ => None,
            },
            Screen::Working { stop_asked, .. } => match (key.code, *stop_asked) {
                (KeyCode::Esc | KeyCode::Char('q'), false) => {
                    *stop_asked = true;
                    None
                }
                (KeyCode::Char('y'), true) => Some(Request::Stop),
                (_, true) => {
                    *stop_asked = false;
                    None
                }
                _ => None,
            },
            Screen::Finished(result) => match key.code {
                KeyCode::Char('e') if matches!(result, Ok(Outcome::Verified)) => {
                    self.selected().map(|t| Request::Eject(t.disk.name.clone()))
                }
                KeyCode::Char('q') | KeyCode::Esc => {
                    self.quit = true;
                    None
                }
                KeyCode::Char('r') | KeyCode::Enter => {
                    self.screen = Screen::Choose;
                    Some(Request::Rescan)
                }
                _ => None,
            },
        }
    }

    /// Ctrl-C: quit when nothing is being written, otherwise ask first.
    fn interrupt(&mut self) -> Option<Request> {
        match &mut self.screen {
            Screen::Working { stop_asked, .. } => *stop_asked = true,
            _ => self.quit = true,
        }
        None
    }

    fn choose_key(&mut self, code: KeyCode) -> Option<Request> {
        match code {
            KeyCode::Char('q') | KeyCode::Esc => self.quit = true,
            KeyCode::Up | KeyCode::Char('k') => self.list.select_previous(),
            KeyCode::Down | KeyCode::Char('j') => {
                let last = self.targets.len().saturating_sub(1);
                self.list.select(Some(self.list.selected().map_or(0, |i| (i + 1).min(last))));
            }
            KeyCode::Char('r') => return Some(Request::Rescan),
            KeyCode::Char('b') => match self.checkout {
                Checkout::None => {
                    self.message =
                        Some("Rebuilding needs a Borea checkout: run borea-flash from one.".into());
                }
                Checkout::Current | Checkout::Changed => return Some(Request::Rebuild),
            },
            KeyCode::Char('u') => match self.selected() {
                Some(t) if matches!(t.verdict, Verdict::Mounted(_)) => {
                    return Some(Request::Unmount(t.disk.clone()));
                }
                Some(_) => self.message = Some("Nothing to unmount on this disk.".into()),
                None => {}
            },
            KeyCode::Enter => {
                self.message = match (&self.checksum, self.selected().map(|t| &t.verdict)) {
                    (Checksum::Mismatch(_), _) => Some(
                        "This image does not match its published checksum. Download it again."
                            .into(),
                    ),
                    (Checksum::Checking { .. }, _) => {
                        Some("Still checking the image. One moment.".into())
                    }
                    (Checksum::Failed(why), _) => Some(format!(
                        "The image could not be checked: {why}. Reopen the helper to retry."
                    )),
                    (_, None) => Some("Insert a USB stick, then press r to look again.".into()),
                    (_, Some(Verdict::Ready)) => {
                        self.screen = Screen::Confirm { typed: String::new() };
                        None
                    }
                    (_, Some(Verdict::Mounted(_))) => Some("Unmount it first: press u.".into()),
                    (_, Some(Verdict::Refused(why))) => {
                        Some(format!("This disk can't be used: {why}."))
                    }
                };
            }
            _ => {}
        }
        None
    }

    // ── Rendering ───────────────────────────────────────────────────────────

    pub fn render(&mut self, frame: &mut Frame) {
        let step = match &self.screen {
            Screen::Choose | Screen::Confirm { .. } => Step::Stick,
            Screen::Working { progress, .. } if progress.phase == Phase::Verifying => Step::Verify,
            Screen::Working { .. } => Step::Write,
            Screen::Finished(Ok(Outcome::Verified)) => Step::Done,
            Screen::Finished(_) => Step::Write,
        };
        let stop_note = "Stop now? The stick won't boot until it is written again.";
        let keys: Vec<(&str, &str)> = match &self.screen {
            Screen::Choose => {
                let mut keys =
                    vec![("↑↓", "choose"), ("Enter", "write"), ("u", "unmount"), ("r", "rescan")];
                if self.checkout != Checkout::None {
                    keys.push(("b", "rebuild"));
                }
                keys.push(("q", "quit"));
                keys
            }
            Screen::Confirm { .. } => vec![("Enter", "erase and write"), ("Esc", "back")],
            Screen::Working { stop_asked: true, .. } => {
                vec![("y", "stop"), ("any other key", "keep going")]
            }
            Screen::Working { .. } => vec![("Esc", "stop")],
            Screen::Finished(Ok(Outcome::Verified)) => {
                vec![("e", "eject"), ("r", "write another"), ("q", "quit")]
            }
            Screen::Finished(_) => vec![("r", "try again"), ("q", "quit")],
        };
        let note = match &self.screen {
            Screen::Working { stop_asked: true, .. } => Some(stop_note.to_owned()),
            _ => self.message.clone(),
        };
        let body = ui::screen(frame, step, note.as_deref(), &keys);
        match &self.screen {
            Screen::Choose => self.render_choose(frame, body),
            Screen::Confirm { typed } => self.render_confirm(frame, body, typed),
            Screen::Working { progress, phase_started, .. } => {
                self.render_working(frame, body, *progress, phase_started.elapsed())
            }
            Screen::Finished(result) => self.render_finished(frame, body, result),
        }
    }

    fn render_choose(&mut self, frame: &mut Frame, area: Rect) {
        let inner = area.width.saturating_sub(4);
        let label = self.image.label.clone().unwrap_or_else(|| "no volume label".into());
        let mut image: Vec<Line> = vec![Line::from(vec![
            Span::raw(self.image.name()).bold(),
            Span::styled(format!("  ·  {}  ·  {label}", human(self.image.size)), SUB),
        ])];
        let (check, style) = match &self.checksum {
            Checksum::Unpublished => {
                ("no published checksum beside it".to_owned(), Style::new().fg(SUB))
            }
            Checksum::Checking { done } => (
                format!("checking SHA-256… {}%", percent(*done, self.image.size)),
                Style::new().fg(SUB),
            ),
            Checksum::Matches(from) => (
                format!(
                    "✓ SHA-256 matches {}",
                    from.file_name().unwrap_or_default().to_string_lossy()
                ),
                Style::new().fg(ACCENT),
            ),
            Checksum::Mismatch(from) => (
                format!("✕ SHA-256 does not match {}: do not use this image", from.display()),
                Style::new().fg(WARN),
            ),
            Checksum::Failed(why) => (format!("could not check: {why}"), Style::new().fg(WARN)),
        };
        image.extend(ui::text(&check, inner, style));
        match self.checkout {
            Checkout::None => {}
            Checkout::Current => image.extend(ui::text(
                "up to date with this checkout · b rebuilds anyway",
                inner,
                Style::new().fg(SUB),
            )),
            Checkout::Changed => image.extend(ui::text(
                "↻ the checkout changed since this image was built · press b to rebuild",
                inner,
                Style::new().fg(WARN),
            )),
        }
        let [_, image_area, _, list_area] = Layout::vertical([
            Constraint::Length(1),
            Constraint::Length(image.len() as u16 + 2),
            Constraint::Length(1),
            Constraint::Fill(1),
        ])
        .areas(area);
        frame.render_widget(Paragraph::new(image).block(ui::panel(" Image ")), image_area);

        let items: Vec<ListItem> = if self.targets.is_empty() {
            vec![ListItem::new(vec![
                Line::from(Span::styled("No USB sticks found.", Style::new().bold())),
                Line::from(Span::styled("Insert one, then press r to look again.", SUB)),
            ])]
        } else {
            self.targets
                .iter()
                .map(|t| {
                    let (mark, style) = match t.verdict {
                        Verdict::Ready => ("●", Style::new()),
                        Verdict::Mounted(_) => ("◐", Style::new().fg(SUB)),
                        Verdict::Refused(_) => {
                            ("○", Style::new().fg(SUB).add_modifier(Modifier::DIM))
                        }
                    };
                    let detail = match t.verdict {
                        Verdict::Mounted(_) => format!("{} · press u to unmount", t.verdict),
                        _ => t.verdict.to_string(),
                    };
                    let mut lines = vec![Line::from(vec![
                        Span::styled(format!("{mark} "), ACCENT),
                        Span::styled(t.describe(), style),
                    ])];
                    lines.extend(
                        ui::wrap(&detail, inner.saturating_sub(4))
                            .into_iter()
                            .map(|l| Line::from(Span::styled(format!("  {l}"), SUB))),
                    );
                    ListItem::new(lines)
                })
                .collect()
        };
        let list = List::new(items)
            .block(ui::panel(" USB stick "))
            .highlight_style(Style::new().bg(SELECTED))
            .highlight_symbol("› ");
        frame.render_stateful_widget(list, list_area, &mut self.list);
    }

    fn render_confirm(&self, frame: &mut Frame, area: Rect, typed: &str) {
        let Some(target) = self.selected() else { return };
        let name = &target.disk.name;
        let width = area.width;
        let mut lines = vec![
            Line::from(""),
            Line::from(Span::styled("Erase and write", Style::new().fg(WARN).bold())),
            Line::from(""),
        ];
        lines.extend(ui::text(
            "Everything on this stick will be erased.",
            width,
            Style::new().bold(),
        ));
        lines.push(Line::from(""));
        lines.push(Line::from(vec![Span::styled("● ", WARN), Span::raw(target.describe())]));
        lines.push(Line::from(Span::styled(
            format!("  becomes {} ({})", self.image.name(), human(self.image.size)),
            SUB,
        )));
        lines.push(Line::from(""));
        lines.push(Line::from(vec![
            Span::raw("Type "),
            Span::styled(name.clone(), Style::new().fg(ACCENT).bold()),
            Span::raw(" to confirm:"),
        ]));
        let height = lines.len() as u16;
        let [text, input] =
            Layout::vertical([Constraint::Length(height), Constraint::Length(3)]).areas(area);
        frame.render_widget(Paragraph::new(lines), text);
        let [input] = Layout::horizontal([Constraint::Length(width.min(40))]).areas(input);
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(format!("{typed}▏"), ACCENT)))
                .block(ui::panel("").border_style(Style::new().fg(WARN))),
            input,
        );
    }

    fn render_working(&self, frame: &mut Frame, area: Rect, p: Progress, elapsed: Duration) {
        let width = area.width;
        let target = self.selected().map(Target::describe).unwrap_or_default();
        let (verb, about) = match p.phase {
            // Nothing written yet: the system may be asking for a password.
            Phase::Writing if p.done == 0 => (
                "Writing to",
                "Waiting for permission to write the stick. If a password prompt appears, enter your password there.",
            ),
            Phase::Writing => ("Writing to", "Writing the image to the stick."),
            Phase::Syncing => (
                "Finishing the write on",
                "Waiting for the stick to confirm. Slow sticks can take a minute here. Don't unplug it.",
            ),
            Phase::Verifying => {
                ("Verifying", "Reading every byte back and comparing it with the image.")
            }
        };
        let mut lines = vec![
            Line::from(""),
            Line::from(vec![Span::raw(format!("{verb} ")).bold(), Span::styled(target, SUB)]),
            Line::from(""),
        ];
        let ratio = if p.total == 0 { 0.0 } else { p.done as f64 / p.total as f64 };
        lines.push(Line::from(format!(
            "{} of {} · {}%",
            human(p.done),
            human(p.total),
            percent(p.done, p.total)
        )));
        lines.push(ui::bar(ratio, width));
        let rate = p.done as f64 / elapsed.as_secs_f64().max(0.5);
        if p.phase != Phase::Syncing && rate > 0.0 {
            let left = p.total.saturating_sub(p.done) as f64 / rate;
            lines.push(Line::from(Span::styled(
                format!("{}/s · about {} left", human(rate as u64), ui::eta(left)),
                SUB,
            )));
        } else {
            lines.push(Line::from(""));
        }
        lines.push(Line::from(""));
        lines.extend(ui::text(about, width, Style::new().fg(SUB)));
        frame.render_widget(Paragraph::new(lines), area);
    }

    fn render_finished(&self, frame: &mut Frame, area: Rect, result: &Result<Outcome, String>) {
        let width = area.width;
        let target = self.selected().map(Target::describe).unwrap_or_default();
        let (heading, style, body, hint) = match result {
            Ok(Outcome::Verified) => (
                "✓ Borea Live is ready.",
                Style::new().fg(ACCENT).bold(),
                format!("Written and verified byte for byte on {target}."),
                "Boot from it, and Borea takes it from there.",
            ),
            Ok(Outcome::Cancelled(_)) => (
                "Stopped.",
                Style::new().fg(WARN).bold(),
                format!("{target} holds part of the image and won't boot."),
                "Write it again whenever you like.",
            ),
            Err(why) => (
                "✕ That didn't work.",
                Style::new().fg(WARN).bold(),
                why.clone(),
                "Nothing else on your computer was touched. Try again, or try another stick.",
            ),
        };
        let mut lines =
            vec![Line::from(""), Line::from(Span::styled(heading, style)), Line::from("")];
        lines.extend(ui::text(&body, width, Style::new()));
        lines.extend(ui::text(hint, width, Style::new().fg(SUB)));
        frame.render_widget(Paragraph::new(lines), area);
    }
}

fn first_ready(targets: &[Target]) -> usize {
    targets.iter().position(|t| t.verdict == Verdict::Ready).unwrap_or(0)
}

fn percent(done: u64, total: u64) -> u64 {
    (done * 100).checked_div(total).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use borea_hardware::{DiskTransport, DiskUsage};
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::crossterm::event::{KeyEventKind, KeyEventState};

    fn stick(name: &str, model: &str, verdict: Verdict) -> Target {
        Target {
            disk: Disk {
                name: name.into(),
                model: Some(model.into()),
                size_bytes: 32_000_000_000,
                removable: true,
                rotational: None,
                transport: DiskTransport::Usb,
                partitions: vec![],
                usage: DiskUsage::Free,
            },
            verdict,
        }
    }

    fn app(targets: Vec<Target>) -> App {
        let image = Image {
            path: "/tmp/borea-live.iso".into(),
            size: 2_100_000_000,
            label: Some("BOREA_LIVE".into()),
            hybrid: true,
        };
        App::new(image, Checksum::Unpublished, targets)
    }

    fn press(app: &mut App, code: KeyCode) -> Option<Request> {
        app.key(KeyEvent {
            code,
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        })
    }

    fn type_text(app: &mut App, text: &str) {
        for c in text.chars() {
            press(app, KeyCode::Char(c));
        }
    }

    fn screen_text(app: &mut App) -> String {
        let mut terminal = Terminal::new(TestBackend::new(90, 28)).unwrap();
        terminal.draw(|f| app.render(f)).unwrap();
        let buffer = terminal.backend().buffer().clone();
        buffer
            .content
            .chunks(90)
            .map(|row| row.iter().map(|c| c.symbol()).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn writing_needs_the_name_typed_back() {
        let mut app = app(vec![stick("sdb", "SanDisk Ultra", Verdict::Ready)]);
        assert!(screen_text(&mut app).contains("SanDisk Ultra · sdb · 32.0 GB"));
        assert_eq!(press(&mut app, KeyCode::Enter), None);
        assert!(screen_text(&mut app).contains("Everything on this stick will be erased."));

        type_text(&mut app, "sda");
        assert_eq!(press(&mut app, KeyCode::Enter), None, "the wrong name never writes");
        assert!(matches!(app.screen, Screen::Confirm { .. }));
        for _ in 0..3 {
            press(&mut app, KeyCode::Backspace);
        }
        type_text(&mut app, "sdb");
        assert_eq!(press(&mut app, KeyCode::Enter), Some(Request::Flash("sdb".into())));
        assert!(matches!(app.screen, Screen::Working { .. }));
    }

    #[test]
    fn refused_and_mounted_sticks_explain_themselves() {
        let mut app = app(vec![
            stick(
                "sda",
                "Old Installer",
                Verdict::Refused("this live session may be running from it".into()),
            ),
            stick("sdb", "Kingston", Verdict::Mounted(vec!["/run/media/me/KINGSTON".into()])),
        ]);
        app.list.select(Some(0));
        press(&mut app, KeyCode::Enter);
        assert!(app.message.as_deref().unwrap().contains("can't be used"));
        assert_eq!(app.screen, Screen::Choose);

        press(&mut app, KeyCode::Down);
        press(&mut app, KeyCode::Enter);
        assert!(app.message.as_deref().unwrap().contains("press u"));
        assert!(
            matches!(press(&mut app, KeyCode::Char('u')), Some(Request::Unmount(d)) if d.name == "sdb")
        );
    }

    #[test]
    fn a_mismatched_image_cannot_be_written() {
        let mut app = app(vec![stick("sdb", "SanDisk", Verdict::Ready)]);
        app.checksum = Checksum::Mismatch("/tmp/SHA256SUMS".into());
        press(&mut app, KeyCode::Enter);
        assert_eq!(app.screen, Screen::Choose);
        assert!(screen_text(&mut app).contains("do not use this image"));
    }

    #[test]
    fn a_failed_checksum_check_cannot_advance_to_erasure_confirmation() {
        let mut app = app(vec![stick("sdb", "SanDisk", Verdict::Ready)]);
        app.checksum = Checksum::Failed("image could not be read".into());
        assert_eq!(press(&mut app, KeyCode::Enter), None);
        assert_eq!(app.screen, Screen::Choose);
        assert!(app.message.as_deref().unwrap().contains("could not be checked"));
    }

    #[test]
    fn stopping_asks_first_and_finishing_offers_eject() {
        let mut app = app(vec![stick("sdb", "SanDisk", Verdict::Ready)]);
        app.progress(Progress { phase: Phase::Writing, done: 700_000_000, total: 2_100_000_000 });
        assert!(screen_text(&mut app).contains("700 MB of 2.1 GB"));
        assert_eq!(press(&mut app, KeyCode::Esc), None);
        assert!(screen_text(&mut app).contains("Stop now?"));
        assert_eq!(press(&mut app, KeyCode::Char('n')), None, "anything but y keeps going");
        press(&mut app, KeyCode::Esc);
        assert_eq!(press(&mut app, KeyCode::Char('y')), Some(Request::Stop));

        app.finished(Ok(Outcome::Verified));
        assert!(screen_text(&mut app).contains("Borea Live is ready."));
        assert_eq!(press(&mut app, KeyCode::Char('e')), Some(Request::Eject("sdb".into())));
    }

    /// `BOREA_FLASH_DUMP=1 cargo test -p borea-flash dump -- --nocapture` prints each screen.
    #[test]
    fn dump_screens() {
        if std::env::var_os("BOREA_FLASH_DUMP").is_none() {
            return;
        }
        let mut app = app(vec![
            stick("sdb", "SanDisk Ultra", Verdict::Ready),
            stick(
                "sdc",
                "Kingston DataTraveler",
                Verdict::Mounted(vec!["/run/media/me/KINGSTON".into()]),
            ),
            stick(
                "sda",
                "Old Installer",
                Verdict::Refused("this live session may be running from it".into()),
            ),
        ]);
        app.checksum = Checksum::Matches("/tmp/borea-live.iso.sha256".into());
        println!("{}\n", screen_text(&mut app));
        press(&mut app, KeyCode::Enter);
        type_text(&mut app, "sd");
        println!("{}\n", screen_text(&mut app));
        type_text(&mut app, "b");
        press(&mut app, KeyCode::Enter);
        app.progress(Progress { phase: Phase::Writing, done: 1_300_000_000, total: 2_100_000_000 });
        println!("{}\n", screen_text(&mut app));
        app.finished(Ok(Outcome::Verified));
        println!("{}", screen_text(&mut app));
    }

    #[test]
    fn rescanning_keeps_the_chosen_stick() {
        let mut app =
            app(vec![stick("sdb", "A", Verdict::Ready), stick("sdc", "B", Verdict::Ready)]);
        app.list.select(Some(1));
        app.set_targets(vec![
            stick("sda", "New", Verdict::Ready),
            stick("sdb", "A", Verdict::Ready),
            stick("sdc", "B", Verdict::Ready),
        ]);
        assert_eq!(app.selected().unwrap().disk.name, "sdc");
    }

    #[test]
    fn rebuilds_only_from_a_checkout_and_says_when_it_is_behind() {
        let mut lone = app(vec![stick("sdb", "Stick", Verdict::Ready)]);
        assert_eq!(press(&mut lone, KeyCode::Char('b')), None);
        assert!(lone.message.as_deref().unwrap().contains("Borea checkout"));
        assert!(!screen_text(&mut lone).contains("b rebuild"));

        let mut behind = app(vec![stick("sdb", "Stick", Verdict::Ready)]);
        behind.checkout = Checkout::Changed;
        let text = screen_text(&mut behind);
        assert!(text.contains("press b to rebuild"), "{text}");
        assert!(text.contains("b rebuild"));
        assert_eq!(press(&mut behind, KeyCode::Char('b')), Some(Request::Rebuild));

        let mut current = app(vec![]);
        current.checkout = Checkout::Current;
        assert!(screen_text(&mut current).contains("up to date with this checkout"));
    }
}
