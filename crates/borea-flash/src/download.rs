//! Get the published Borea Live ISO instead of building it.
//!
//! Every GitHub release carries `borea-live-x86_64.iso` and its `.sha256`. The
//! image is downloaded into the user's cache, resuming an interrupted download,
//! checked against that SHA-256, and kept: the next run only fetches the small
//! checksum file, and downloads again only when a newer release has a different
//! image.

use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use ratatui::Frame;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Gauge, Paragraph, Wrap};
use sha2::{Digest, Sha256};

/// The latest release's files. `BOREA_RELEASES_URL` points elsewhere (tests, mirrors).
pub const RELEASES: &str = "https://github.com/shaddick-dev/borea-live/releases/latest/download";
/// The ISO's name in every release.
pub const ISO: &str = "borea-live-x86_64.iso";

const ACCENT: Color = Color::Rgb(0xa2, 0xc4, 0xa8);
const SUB: Color = Color::Rgb(0x8a, 0x96, 0x8c);
const WARN: Color = Color::Rgb(0xe3, 0x9b, 0x84);

fn releases() -> String {
    #[cfg(test)]
    if let Some(url) = tests::TEST_URL.lock().ok().and_then(|url| url.clone()) {
        return url;
    }
    std::env::var("BOREA_RELEASES_URL").unwrap_or_else(|_| RELEASES.to_owned())
}

/// Where downloaded images live: `$XDG_CACHE_HOME/borea` or `~/.cache/borea`.
pub fn cache_dir() -> Option<PathBuf> {
    std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cache")))
        .map(|dir| dir.join("borea"))
}

fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_connect(Some(Duration::from_secs(20)))
        // A stalled connection fails instead of hanging; a slow one carries on.
        .timeout_per_read(Some(Duration::from_secs(60)))
        .user_agent(concat!("borea-rataTUI/", env!("CARGO_PKG_VERSION")))
        .build()
        .into()
}

fn describe(error: ureq::Error) -> String {
    match error {
        ureq::Error::StatusCode(404) => {
            "No Borea Live release has been published yet. Build one from source with `borea-rataTUI --build` (takes hours).".into()
        }
        ureq::Error::StatusCode(code) => format!("the release server answered HTTP {code}"),
        ureq::Error::Io(e) => format!("the download was interrupted: {e}"),
        other => format!("could not reach the release server: {other}"),
    }
}

/// The SHA-256 the latest release publishes for its ISO.
pub fn latest_checksum() -> Result<String, String> {
    let url = format!("{}/{ISO}.sha256", releases());
    let mut response = agent().get(&url).call().map_err(describe)?;
    let text = response
        .body_mut()
        .with_config()
        .limit(4096)
        .read_to_string()
        .map_err(|e| format!("could not read the published checksum: {e}"))?;
    parse_checksum(&text).ok_or_else(|| "the published checksum file is malformed".to_owned())
}

/// The 64-hex-digit SHA-256 at the start of a `sha256sum` line.
pub fn parse_checksum(text: &str) -> Option<String> {
    let first = text.split_whitespace().next()?;
    (first.len() == 64 && first.bytes().all(|b| b.is_ascii_hexdigit()))
        .then(|| first.to_ascii_lowercase())
}

fn hex(digest: &[u8]) -> String {
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

/// What a download reports as it goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    /// Comparing the cached image with the published checksum.
    Checking,
    /// Bytes received so far, and the image's size when the server says.
    Receiving { done: u64, total: Option<u64> },
    /// Checking the finished download.
    Verifying,
}

/// Download the latest release's ISO into `dir`, or reuse the copy already
/// there when it matches the published checksum. Returns the image's path, with
/// its `.sha256` beside it so the writer checks it again before writing.
pub fn fetch(
    dir: &Path,
    cancel: &AtomicBool,
    mut progress: impl FnMut(Step),
) -> Result<PathBuf, String> {
    fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let expected = latest_checksum()?;
    let image = dir.join(ISO);
    let sidecar = dir.join(format!("{ISO}.sha256"));
    let part = dir.join(format!("{ISO}.part"));
    let publish = || {
        fs::write(&sidecar, format!("{expected}  {ISO}\n"))
            .map_err(|e| format!("{}: {e}", sidecar.display()))
    };

    if image.is_file() {
        progress(Step::Checking);
        if crate::image::sha256(&image, |_| {}).is_ok_and(|sha| sha == expected) {
            publish()?;
            return Ok(image);
        }
    }

    // Resume a partial download; the server's answer says whether it can.
    let mut hasher = Sha256::new();
    let mut have = 0u64;
    if part.is_file() {
        progress(Step::Checking);
        let mut file = File::open(&part).map_err(|e| e.to_string())?;
        let mut buf = vec![0u8; 1 << 20];
        loop {
            let n = file.read(&mut buf).map_err(|e| e.to_string())?;
            if n == 0 {
                break;
            }
            hasher.update(&buf[..n]);
            have += n as u64;
        }
    }
    let url = format!("{}/{ISO}", releases());
    let mut request = agent().get(&url);
    if have > 0 {
        request = request.header("Range", format!("bytes={have}-"));
    }
    let mut response = request.call().map_err(describe)?;
    let resumed = have > 0 && response.status().as_u16() == 206;
    if !resumed {
        hasher = Sha256::new();
        have = 0;
    }
    let length = response
        .headers()
        .get("content-length")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<u64>().ok());
    let total = length.map(|n| n + have);
    let mut out = OpenOptions::new()
        .create(true)
        .write(true)
        .append(resumed)
        .truncate(!resumed)
        .open(&part)
        .map_err(|e| format!("{}: {e}", part.display()))?;

    let mut reader = response.body_mut().as_reader();
    let mut buf = vec![0u8; 1 << 20];
    let mut done = have;
    progress(Step::Receiving { done, total });
    loop {
        if cancel.load(Ordering::Relaxed) {
            let _ = out.flush();
            return Err("Download stopped. Running again picks up where it left off.".into());
        }
        let n = reader.read(&mut buf).map_err(|e| {
            format!("the download was interrupted: {e}. Running again picks up where it left off.")
        })?;
        if n == 0 {
            break;
        }
        out.write_all(&buf[..n]).map_err(|e| format!("{}: {e}", part.display()))?;
        hasher.update(&buf[..n]);
        done += n as u64;
        progress(Step::Receiving { done, total });
    }
    out.sync_all().map_err(|e| e.to_string())?;
    drop(out);

    progress(Step::Verifying);
    let actual = hex(&hasher.finalize());
    if actual != expected {
        let _ = fs::remove_file(&part);
        return Err(format!(
            "the downloaded image doesn't match the published SHA-256 (got {actual}); it was deleted, try again"
        ));
    }
    fs::rename(&part, &image).map_err(|e| e.to_string())?;
    publish()?;
    Ok(image)
}

/// What the download screen asks the main loop to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DownloadRequest {
    Stop,
    Retry,
    Quit,
}

pub struct DownloadView {
    started: Instant,
    step: Option<Step>,
    failed: Option<String>,
    stop_asked: bool,
}

impl Default for DownloadView {
    fn default() -> Self {
        Self::new()
    }
}

impl DownloadView {
    pub fn new() -> Self {
        DownloadView { started: Instant::now(), step: None, failed: None, stop_asked: false }
    }

    pub fn step(&mut self, step: Step) {
        if !matches!(self.step, Some(Step::Receiving { .. }))
            || matches!(step, Step::Receiving { .. } | Step::Verifying)
        {
            self.step = Some(step);
        }
    }

    pub fn failed(&mut self, why: String) {
        self.failed = Some(why);
        self.stop_asked = false;
    }

    pub fn key(&mut self, key: KeyEvent) -> Option<DownloadRequest> {
        let ctrl_c =
            key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c');
        if self.failed.is_some() {
            return match key.code {
                KeyCode::Char('r') | KeyCode::Enter => {
                    self.failed = None;
                    self.step = None;
                    self.started = Instant::now();
                    Some(DownloadRequest::Retry)
                }
                KeyCode::Char('q') | KeyCode::Esc => Some(DownloadRequest::Quit),
                _ if ctrl_c => Some(DownloadRequest::Quit),
                _ => None,
            };
        }
        match (key.code, self.stop_asked) {
            (KeyCode::Char('y'), true) => Some(DownloadRequest::Stop),
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
        let [_, title, status, _, bar, _, ask] = Layout::vertical([
            Constraint::Length(1),
            Constraint::Length(2),
            Constraint::Length(2),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Fill(1),
            Constraint::Length(3),
        ])
        .areas(area);

        let heading = match &self.failed {
            Some(_) => Span::styled("✕ The download stopped.", Style::new().fg(WARN).bold()),
            None => Span::raw("Getting Borea Live").bold(),
        };
        frame.render_widget(Paragraph::new(Line::from(heading)), title);

        let elapsed = self.started.elapsed().as_secs_f64().max(0.001);
        let status_line = match (&self.failed, self.step) {
            (Some(why), _) => Line::from(Span::styled(why.clone(), WARN)),
            (None, None) => Line::from(Span::styled("Looking up the latest release…", SUB)),
            (None, Some(Step::Checking)) => {
                Line::from(Span::styled("Checking the copy you already have…", SUB))
            }
            (None, Some(Step::Verifying)) => {
                Line::from(Span::styled("Checking the download against its SHA-256…", SUB))
            }
            (None, Some(Step::Receiving { done, total })) => {
                let rate = done as f64 / elapsed;
                let mut spans = vec![
                    Span::styled("● ", ACCENT),
                    Span::raw(match total {
                        Some(total) => format!("{} of {}", human(done), human(total)),
                        None => human(done),
                    }),
                    Span::styled(format!("   {}/s", human(rate as u64)), SUB),
                ];
                if let Some(total) = total.filter(|_| rate > 1.0) {
                    let left = total.saturating_sub(done) as f64 / rate;
                    spans.push(Span::styled(format!("   about {} left", eta(left)), SUB));
                }
                Line::from(spans)
            }
        };
        let note = Line::from(Span::styled(
            "The ready-made image from the latest release, checked against its published SHA-256 and kept for next time.",
            SUB,
        ));
        frame.render_widget(
            Paragraph::new(vec![status_line, note]).wrap(Wrap { trim: true }),
            status,
        );

        if let Some(Step::Receiving { done, total: Some(total) }) = self.step
            && total > 0
            && self.failed.is_none()
        {
            let ratio = (done as f64 / total as f64).clamp(0.0, 1.0);
            frame.render_widget(
                Gauge::default()
                    .gauge_style(Style::new().fg(ACCENT))
                    .ratio(ratio)
                    .label(format!("{:.0}%", ratio * 100.0)),
                bar,
            );
        }

        let footer = match (&self.failed, self.stop_asked) {
            (Some(_), _) => vec![Line::from(Span::styled("r try again · q quit", SUB))],
            (None, true) => vec![
                Line::from(Span::styled(
                    "Stop the download? What has arrived is kept for next time.",
                    Style::new().fg(WARN).add_modifier(Modifier::BOLD),
                )),
                Line::from(Span::styled("y stop · any other key keep going", SUB)),
            ],
            (None, false) => vec![Line::from(Span::styled(
                "When it's done, Borea moves straight on to writing your USB stick · Esc stop",
                SUB,
            ))],
        };
        frame.render_widget(Paragraph::new(footer), ask);
    }
}

fn human(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["B", "KB", "MB", "GB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1000.0 && unit < UNITS.len() - 1 {
        value /= 1000.0;
        unit += 1;
    }
    if unit == 0 { format!("{bytes} B") } else { format!("{value:.1} {}", UNITS[unit]) }
}

fn eta(seconds: f64) -> String {
    let s = seconds.round() as u64;
    if s >= 3600 {
        format!("{}h {:02}m", s / 3600, s % 3600 / 60)
    } else if s >= 60 {
        format!("{}m {:02}s", s / 60, s % 60)
    } else {
        format!("{s}s")
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::io::{BufRead, BufReader};
    use std::net::TcpListener;
    use std::sync::{Arc, Mutex};

    #[test]
    fn checksums_are_parsed_from_sha256sum_lines() {
        let sha = "a".repeat(64);
        assert_eq!(parse_checksum(&format!("{sha}  borea-live-x86_64.iso\n")), Some(sha));
        assert_eq!(parse_checksum("nonsense"), None);
        assert_eq!(parse_checksum(""), None);
    }

    /// A tiny HTTP server for one test: serves `files`, honours a Range start,
    /// and records every request line.
    fn serve(files: Vec<(&'static str, Vec<u8>)>) -> (String, Arc<Mutex<Vec<String>>>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let seen = Arc::new(Mutex::new(Vec::new()));
        let log = Arc::clone(&seen);
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let mut stream = stream.unwrap();
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut first = String::new();
                reader.read_line(&mut first).unwrap();
                let mut range = None;
                loop {
                    let mut header = String::new();
                    reader.read_line(&mut header).unwrap();
                    if header.trim().is_empty() {
                        break;
                    }
                    if let Some(v) = header.to_ascii_lowercase().strip_prefix("range: bytes=") {
                        range = v.trim().trim_end_matches('-').parse::<usize>().ok();
                    }
                }
                log.lock().unwrap().push(first.trim().to_owned());
                let path = first.split_whitespace().nth(1).unwrap_or("/");
                let found = files.iter().find(|(name, _)| path == format!("/{name}"));
                let reply = match (found, range) {
                    (Some((_, body)), Some(start)) => {
                        let rest = &body[start..];
                        let mut r = format!(
                            "HTTP/1.1 206 Partial Content\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                            rest.len()
                        )
                        .into_bytes();
                        r.extend_from_slice(rest);
                        r
                    }
                    (Some((_, body)), None) => {
                        let mut r = format!(
                            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                            body.len()
                        )
                        .into_bytes();
                        r.extend_from_slice(body);
                        r
                    }
                    (None, _) => {
                        b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                            .to_vec()
                    }
                };
                let _ = stream.write_all(&reply);
            }
        });
        (base, seen)
    }

    fn iso_bytes() -> Vec<u8> {
        (0..3_000_000u32).map(|i| (i % 251) as u8).collect()
    }

    fn sha(bytes: &[u8]) -> String {
        hex(&Sha256::digest(bytes))
    }

    /// The release URL is shared, so these run as one test.
    #[test]
    fn downloads_verifies_caches_resumes_and_refuses_bad_images() {
        let iso = iso_bytes();
        let good = format!("{}  {ISO}\n", sha(&iso));
        let (base, seen) = serve(vec![
            ("good/borea-live-x86_64.iso", iso.clone()),
            ("good/borea-live-x86_64.iso.sha256", good.clone().into_bytes()),
            ("bad/borea-live-x86_64.iso", vec![7u8; 1000]),
            ("bad/borea-live-x86_64.iso.sha256", good.into_bytes()),
        ]);
        let dir = crate::image::tests::temp_dir("download");
        let cancel = AtomicBool::new(false);

        use_releases(&format!("{base}/good"));
        let path = fetch(&dir, &cancel, |_| {}).unwrap();
        assert_eq!(fs::read(&path).unwrap(), iso);
        assert!(
            fs::read_to_string(dir.join(format!("{ISO}.sha256"))).unwrap().contains(&sha(&iso))
        );

        // A matching cached copy only costs the checksum request.
        let before = seen.lock().unwrap().len();
        fetch(&dir, &cancel, |_| {}).unwrap();
        let requests = seen.lock().unwrap()[before..].to_vec();
        assert_eq!(requests, [format!("GET /good/{ISO}.sha256 HTTP/1.1")]);

        // An interrupted download resumes from its partial file.
        fs::remove_file(&path).unwrap();
        fs::write(dir.join(format!("{ISO}.part")), &iso[..1_000_000]).unwrap();
        let path = fetch(&dir, &cancel, |_| {}).unwrap();
        assert_eq!(fs::read(&path).unwrap(), iso);

        // An image that doesn't match its checksum is refused and deleted.
        let other = crate::image::tests::temp_dir("download-bad");
        use_releases(&format!("{base}/bad"));
        let error = fetch(&other, &cancel, |_| {}).unwrap_err();
        assert!(error.contains("doesn't match"), "{error}");
        assert!(!other.join(ISO).exists() && !other.join(format!("{ISO}.part")).exists());

        // No release yet: a plain explanation, not an HTTP error.
        use_releases(&format!("{base}/none"));
        let error = fetch(&other, &cancel, |_| {}).unwrap_err();
        assert!(error.contains("No Borea Live release has been published yet"), "{error}");
    }

    /// Point downloads at the test server (the environment variable would need
    /// `unsafe` in Rust 2024, which this workspace forbids).
    fn use_releases(url: &str) {
        *TEST_URL.lock().unwrap() = Some(url.to_owned());
    }

    pub(crate) static TEST_URL: Mutex<Option<String>> = Mutex::new(None);
}
