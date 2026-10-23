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
use ratatui::layout::{Constraint, Flex, Layout, Rect};
use ratatui::style::{Color, Modifier, Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Block, BorderType, Borders, Clear, Gauge, List, ListItem, ListState, Padding, Paragraph, Wrap,
};

use crate::image::Image;
use crate::target::{Target, Verdict, human};
use crate::write::{Outcome, Phase, Progress};

/// Borea's dark palette, for terminals.
const ACCENT: Color = Color::Rgb(0xa2, 0xc4, 0xa8);
const SUB: Color = Color::Rgb(0x8a, 0x96, 0x8c);
const WARN: Color = Color::Rgb(0xe3, 0x9b, 0x84);

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
        let [header, body, footer] =
            Layout::vertical([Constraint::Length(4), Constraint::Fill(1), Constraint::Length(2)])
                .areas(frame.area());
        render_header(frame, header);
        match &self.screen {
            Screen::Choose | Screen::Confirm { .. } => self.render_choose(frame, body),
            Screen::Working { progress, phase_started, stop_asked } => {
                self.render_working(frame, body, *progress, phase_started.elapsed(), *stop_asked)
            }
            Screen::Finished(result) => self.render_finished(frame, body, result),
        }
        if let Screen::Confirm { typed } = &self.screen {
            self.render_confirm(frame, body, typed);
        }
        self.render_footer(frame, footer);
    }

    fn render_choose(&mut self, frame: &mut Frame, area: Rect) {
        let [image_area, list_area] =
            Layout::vertical([Constraint::Length(6), Constraint::Fill(1)]).areas(area);
        let label = self.image.label.clone().unwrap_or_else(|| "no volume label".into());
        let check = match &self.checksum {
            Checksum::Unpublished => {
                Line::from(Span::styled("no published checksum beside it", SUB))
            }
            Checksum::Checking { done } => Line::from(Span::styled(
                format!("checking SHA-256… {}%", percent(*done, self.image.size)),
                SUB,
            )),
            Checksum::Matches(from) => Line::from(Span::styled(
                format!(
                    "✓ SHA-256 matches {}",
                    from.file_name().unwrap_or_default().to_string_lossy()
                ),
                ACCENT,
            )),
            Checksum::Mismatch(from) => Line::from(Span::styled(
                format!("✕ SHA-256 does not match {}: do not use this image", from.display()),
                WARN,
            )),
            Checksum::Failed(why) => {
                Line::from(Span::styled(format!("could not check: {why}"), WARN))
            }
        };
        let image = Paragraph::new(vec![
            Line::from(vec![
                Span::raw(self.image.name()).bold(),
                Span::styled(format!("  ·  {label}"), SUB),
            ]),
            Line::from(Span::styled(
                format!("{} · bootable from USB", human(self.image.size)),
                SUB,
            )),
            check,
            match self.checkout {
                Checkout::None => Line::from(""),
                Checkout::Current => Line::from(Span::styled(
                    "up to date with this checkout · b rebuilds anyway",
                    SUB,
                )),
                Checkout::Changed => Line::from(Span::styled(
                    "↻ the checkout changed since this image was built · press b to rebuild",
                    WARN,
                )),
            },
        ])
        .block(panel(" Image "));
        frame.render_widget(image, image_area);

        let items: Vec<ListItem> = if self.targets.is_empty() {
            vec![ListItem::new(Line::from(Span::styled(
                "No USB sticks found. Insert one, then press r.",
                SUB,
            )))]
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
                    ListItem::new(vec![
                        Line::from(vec![
                            Span::styled(format!("{mark} "), ACCENT),
                            Span::styled(t.describe(), style),
                        ]),
                        Line::from(Span::styled(
                            match t.verdict {
                                Verdict::Mounted(_) => {
                                    format!("  {} · press u to unmount", t.verdict)
                                }
                                _ => format!("  {}", t.verdict),
                            },
                            SUB,
                        )),
                    ])
                })
                .collect()
        };
        let list = List::new(items)
            .block(panel(" USB stick "))
            .highlight_style(Style::new().bg(Color::Rgb(0x2d, 0x3b, 0x30)))
            .highlight_symbol("› ");
        frame.render_stateful_widget(list, list_area, &mut self.list);
    }

    fn render_confirm(&self, frame: &mut Frame, area: Rect, typed: &str) {
        let Some(target) = self.selected() else { return };
        let box_area = centered(area, 64, 11);
        frame.render_widget(Clear, box_area);
        let name = &target.disk.name;
        let text = vec![
            Line::from(Span::styled(
                "Everything on this stick will be erased.",
                Style::new().fg(WARN).bold(),
            )),
            Line::from(""),
            Line::from(target.describe()),
            Line::from(Span::styled(format!("will become {}", self.image.name()), SUB)),
            Line::from(""),
            Line::from(vec![
                Span::raw("Type "),
                Span::raw(name).bold(),
                Span::raw(" to confirm: "),
            ]),
            Line::from(Span::styled(format!("› {typed}▏"), ACCENT)),
        ];
        let dialog = Paragraph::new(text).wrap(Wrap { trim: false }).block(
            Block::new()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::new().fg(WARN))
                .title(" Erase and write ")
                .padding(Padding::horizontal(2)),
        );
        frame.render_widget(dialog, box_area);
    }

    fn render_working(
        &self,
        frame: &mut Frame,
        area: Rect,
        p: Progress,
        elapsed: Duration,
        stop_asked: bool,
    ) {
        let area = area.inner(ratatui::layout::Margin { horizontal: 2, vertical: 0 });
        let [_, title, gauge, detail, _, ask] = Layout::vertical([
            Constraint::Length(1),
            Constraint::Length(2),
            Constraint::Length(3),
            Constraint::Length(2),
            Constraint::Fill(1),
            Constraint::Length(3),
        ])
        .areas(area);
        let target = self.selected().map(Target::describe).unwrap_or_default();
        let verb = match p.phase {
            Phase::Writing => "Writing",
            Phase::Syncing => "Finishing the write",
            Phase::Verifying => "Verifying",
        };
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::raw(format!("{verb} ")).bold(),
                Span::styled(target, SUB),
            ])),
            title,
        );
        let ratio = if p.total == 0 { 0.0 } else { p.done as f64 / p.total as f64 };
        let label = match p.phase {
            Phase::Syncing => "waiting for the stick to confirm…".to_owned(),
            _ => format!("{} of {}", human(p.done), human(p.total)),
        };
        frame.render_widget(
            Gauge::default()
                .block(panel(""))
                .gauge_style(Style::new().fg(ACCENT))
                .ratio(ratio.min(1.0))
                .label(label),
            gauge,
        );
        let rate = p.done as f64 / elapsed.as_secs_f64().max(0.5);
        let line = match p.phase {
            Phase::Syncing => "Slow sticks can take a minute here. Don't unplug it.".to_owned(),
            _ if rate > 0.0 => {
                let left = (p.total.saturating_sub(p.done)) as f64 / rate;
                format!("{}/s · about {} left", human(rate as u64), clock(left as u64))
            }
            _ => String::new(),
        };
        frame.render_widget(Paragraph::new(Span::styled(line, SUB)), detail);
        if stop_asked {
            frame.render_widget(
                Paragraph::new(vec![
                    Line::from(Span::styled(
                        "Stop now? The stick won't boot until it is written again.",
                        Style::new().fg(WARN),
                    )),
                    Line::from(Span::styled("y stop · any other key keep going", SUB)),
                ]),
                ask,
            );
        }
    }

    fn render_finished(&self, frame: &mut Frame, area: Rect, result: &Result<Outcome, String>) {
        let target = self.selected().map(Target::describe).unwrap_or_default();
        let lines = match result {
            Ok(Outcome::Verified) => vec![
                Line::from(Span::styled("✓ Borea Live is ready.", Style::new().fg(ACCENT).bold())),
                Line::from(""),
                Line::from(format!("Written and verified byte for byte on {target}.")),
                Line::from(Span::styled("Boot from it, and Borea takes it from there.", SUB)),
            ],
            Ok(Outcome::Cancelled(_)) => vec![
                Line::from(Span::styled("Stopped.", Style::new().fg(WARN).bold())),
                Line::from(""),
                Line::from(format!("{target} holds part of the image and won't boot.")),
                Line::from(Span::styled("Write it again whenever you like.", SUB)),
            ],
            Err(why) => vec![
                Line::from(Span::styled("✕ That didn't work.", Style::new().fg(WARN).bold())),
                Line::from(""),
                Line::from(why.clone()),
            ],
        };
        frame
            .render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }).block(panel("")), area);
    }

    fn render_footer(&self, frame: &mut Frame, area: Rect) {
        let keys = match &self.screen {
            Screen::Choose if self.checkout != Checkout::None => {
                "↑↓ choose · Enter write · u unmount · r rescan · b rebuild · q quit"
            }
            Screen::Choose => "↑↓ choose · Enter write · u unmount · r rescan · q quit",
            Screen::Confirm { .. } => "type the name · Enter confirm · Esc back",
            Screen::Working { .. } => "Esc stop",
            Screen::Finished(Ok(Outcome::Verified)) => "e eject · r write another · q quit",
            Screen::Finished(_) => "r try again · q quit",
        };
        let line = match &self.message {
            Some(message) => Line::from(Span::styled(message.clone(), WARN)),
            None => Line::from(Span::styled(keys, SUB)),
        };
        frame.render_widget(
            Paragraph::new(line).block(Block::new().padding(Padding::horizontal(2))),
            area,
        );
    }
}

pub fn render_header(frame: &mut Frame, area: Rect) {
    let lines = vec![
        Line::from(""),
        Line::from(vec![
            Span::raw("  BOREA").bold(),
            Span::styled("  TUI Helper", SUB),
            Span::styled("    Prepare ISO → write USB → verify", SUB),
        ]),
        Line::from(Span::styled(
            "  ∼ ∽ ∼   go wherever the wind takes you",
            Style::new().fg(ACCENT).add_modifier(Modifier::DIM),
        )),
    ];
    frame.render_widget(Paragraph::new(lines), area);
}

fn panel(title: &str) -> Block<'_> {
    Block::new()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::new().fg(Color::Rgb(0x39, 0x44, 0x3b)))
        .title(Span::styled(title, SUB))
        .padding(Padding::horizontal(1))
}

fn first_ready(targets: &[Target]) -> usize {
    targets.iter().position(|t| t.verdict == Verdict::Ready).unwrap_or(0)
}

fn centered(area: Rect, width: u16, height: u16) -> Rect {
    let [row] = Layout::vertical([Constraint::Length(height)]).flex(Flex::Center).areas(area);
    let [cell] = Layout::horizontal([Constraint::Length(width.min(area.width))])
        .flex(Flex::Center)
        .areas(row);
    cell
}

fn percent(done: u64, total: u64) -> u64 {
    (done * 100).checked_div(total).unwrap_or(0)
}

fn clock(seconds: u64) -> String {
    format!("{}:{:02}", seconds / 60, seconds % 60)
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
