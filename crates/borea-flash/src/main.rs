//! borea-rataTUI: prepare Borea Live, write a USB stick, then verify it.
//! (`borea-helper` is its earlier name and still works the same way.)
//!
//! ```text
//! borea-rataTUI                prepare the current ISO, then write a USB stick
//! borea-flash [IMAGE]           write IMAGE, or the newest borea-live*.iso found
//! borea-flash --make            in a Borea checkout: rebuild Borea Live if its sources
//!                               changed since the last image, then write it
//! borea-flash --build           as --make, but always rebuild
//! borea-flash --list [IMAGE]    print the sticks it sees and exit
//! borea-flash --install         install both commands and the app launcher
//! borea-flash --install-launcher
//!                               add a "borea-rataTUI" launcher
//! ```

use std::io::{BufRead, BufReader, Write};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitCode, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use borea_flash::app::{App, Checkout, Checksum, Request};
use borea_flash::download::{self, DownloadRequest, DownloadView};
use borea_flash::image::{self, Image};
use borea_flash::make::{self, BuildRequest, BuildView};
use borea_flash::target::{self, Target, Verdict};
use borea_flash::terminal::{self as screen, Colours};
use borea_flash::write::{self, Outcome, Progress};
use borea_flash::{FlashError, device};
use borea_host::HostKind;
use ratatui::crossterm::event::{self, Event as Term, KeyEventKind};

use borea_flash::install;

/// `println!`, minus the panic when the reader goes away (`borea-flash --list | head`).
macro_rules! say {
    ($($arg:tt)*) => {{
        let _ = writeln!(std::io::stdout(), $($arg)*);
    }};
}

/// Results from worker threads.
enum Event {
    Checksum(Checksum),
    Targets(Vec<Target>),
    Progress(Progress),
    Finished(Result<Outcome, String>),
    Message(String),
    BuildLine(String),
    /// The build ended: the new image, or why it failed.
    Built(Result<PathBuf, String>),
    Download(download::Step),
    /// The download ended: the image, or why it stopped.
    Downloaded(Result<PathBuf, String>),
}

const USAGE: &str = "\
usage: borea-rataTUI            get the latest Borea Live release (a download, kept for
                                next time), then write and verify a USB stick
       borea-rataTUI IMAGE.iso  write an existing ISO
       borea-rataTUI --cached   use an existing local ISO; no download or rebuild
       borea-flash [IMAGE.iso]   write IMAGE, or the newest Borea Live image found,
                                 downloading the latest release when there is none
       borea-flash --make        developers: in a Borea checkout, build Borea Live from source
                                 if it changed (hours the first time), then write it
       borea-flash --build       as --make, but always rebuild
       borea-flash --list        show the USB sticks it could write, and exit
       borea-rataTUI --update    replace this program with the latest release's ready-made
                                 build (seconds; nothing to compile)
       borea-flash --install     install the commands and the borea-rataTUI launcher
       borea-flash --install-launcher
                                 install the commands and the borea-rataTUI launcher";

/// What the screen shows: getting the image (download or build), then the stick.
enum Stage {
    Downloading(DownloadView),
    Building(BuildView),
    Writing(Box<App>),
}

/// Set by the app launcher: keep the terminal open after a failure, so the reason
/// can be read before the launcher's window closes.
static WAIT_ON_ERROR: AtomicBool = AtomicBool::new(false);
const WAIT_FLAG: &str = "--wait-on-error";

pub(crate) fn main() -> ExitCode {
    let code = start();
    if code != ExitCode::SUCCESS && WAIT_ON_ERROR.load(Ordering::Relaxed) {
        eprint!("\nPress Enter to close this window.");
        let _ = std::io::stdin().read_line(&mut String::new());
    }
    code
}

fn start() -> ExitCode {
    let helper = std::env::args_os().next().is_some_and(|p| {
        Path::new(&p).file_stem().is_some_and(|s| s == "borea-rataTUI" || s == "borea-helper")
    });
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    if take_wait_flag(&mut args) {
        WAIT_ON_ERROR.store(true, Ordering::Relaxed);
    }
    let args = arguments(args, helper);
    let list_only = args.iter().any(|a| a == "--list");
    if args.iter().any(|a| a == "-h" || a == "--help") {
        say!("Write the Borea Live ISO to a USB stick, then verify it byte for byte.\n\n{USAGE}");
        return ExitCode::SUCCESS;
    }
    let flag = |name: &str| args.iter().any(|a| a == name);
    if flag("--install") || flag("--install-launcher") {
        return report(install_command().and_then(|helper| install_launcher(&helper)));
    }
    if flag("--update") {
        return report(update());
    }
    if flag("--get") {
        let host = borea_host::detect().kind;
        let start = Stage::Downloading(DownloadView::new());
        return report(run(start, None, host).map_err(|e| e.to_string()));
    }
    if flag("--make") || flag("--build") {
        let Some(repo) = make::repo() else {
            eprintln!("borea-flash: --make works inside a Borea checkout");
            return ExitCode::FAILURE;
        };
        let host = borea_host::detect().kind;
        let newest = make::newest_image(&repo);
        let start = match newest {
            Some(path) if !flag("--build") && !make::needs_build(&repo, Some(&path)) => {
                match writing(&path, &host, Some(&repo)) {
                    Ok(app) => Stage::Writing(Box::new(app)),
                    Err(error) => {
                        eprintln!("borea-flash: {error}");
                        return ExitCode::FAILURE;
                    }
                }
            }
            _ => Stage::Building(BuildView::new(repo.join("target/live/build.log"))),
        };
        return report(run(start, Some(repo), host).map_err(|e| e.to_string()));
    }

    let path = args.iter().find(|a| !a.starts_with("--")).map(PathBuf::from).or_else(find_image);
    let Some(path) = path else {
        if flag("--cached") {
            eprintln!(
                "borea-rataTUI: no cached ISO found. Download a published release or build an image once before using --cached."
            );
            return ExitCode::FAILURE;
        }
        if !list_only {
            // Nothing to write yet: get the latest release, in minutes.
            let host = borea_host::detect().kind;
            let start = Stage::Downloading(DownloadView::new());
            return report(run(start, None, host).map_err(|e| e.to_string()));
        }
        eprintln!(
            "borea-flash: no Borea Live image found. Run borea-rataTUI to download the latest \
             release, or name the .iso to write.\n\n{USAGE}"
        );
        return ExitCode::FAILURE;
    };
    let image = match Image::inspect(&path) {
        Ok(image) => image,
        Err(error) => {
            eprintln!("borea-flash: {error}");
            return ExitCode::FAILURE;
        }
    };
    let host = borea_host::detect().kind;
    let targets = target::discover(Path::new("/"), &image.path, image.size, &host);

    if list_only {
        say!("{} · {}", image.name(), target::human(image.size));
        for t in &targets {
            say!("  {}  {}", t.describe(), t.verdict);
        }
        if targets.is_empty() {
            say!("  no USB sticks found");
        }
        return ExitCode::SUCCESS;
    }

    let app = writing_with(image, targets, None);
    report(run(Stage::Writing(Box::new(app)), None, host).map_err(|e| e.to_string()))
}

/// Remove the launcher's flag, saying whether it was there. It never changes
/// what the command does, so the helper's own default still applies.
fn take_wait_flag(args: &mut Vec<String>) -> bool {
    let present = args.iter().any(|a| a == WAIT_FLAG);
    args.retain(|a| a != WAIT_FLAG);
    present
}

/// The app gets the latest release by default; explicit files and flags keep
/// their own meaning. Building from source (hours) is only ever asked for.
fn arguments(mut args: Vec<String>, helper: bool) -> Vec<String> {
    if helper && args.is_empty() {
        args.push("--get".into());
    }
    args
}

fn report(result: Result<(), String>) -> ExitCode {
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("borea-flash: {error}");
            ExitCode::FAILURE
        }
    }
}

/// The writing stage for an image, with its sticks found.
fn writing(path: &Path, host: &HostKind, repo: Option<&Path>) -> Result<App, String> {
    let image = Image::inspect(path).map_err(|e| e.to_string())?;
    let targets = target::discover(Path::new("/"), &image.path, image.size, host);
    Ok(writing_with(image, targets, repo))
}

/// `borea-live*.iso` in this checkout's build output, the download cache, the
/// current directory, or ~/Downloads, newest first.
fn find_image() -> Option<PathBuf> {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let dirs = [
        make::repo().map(|repo| repo.join("target/live/out")),
        download::cache_dir(),
        Some(PathBuf::from(".")),
        home.map(|h| h.join("Downloads")),
    ];
    let mut found: Vec<(std::time::SystemTime, PathBuf)> = dirs
        .into_iter()
        .flatten()
        .filter_map(|dir| std::fs::read_dir(dir).ok())
        .flatten()
        .filter_map(|entry| {
            let path = entry.ok()?.path();
            let name = path.file_name()?.to_string_lossy().to_lowercase();
            let modified = path.metadata().ok()?.modified().ok()?;
            (name.starts_with("borea-live") && name.ends_with(".iso")).then_some((modified, path))
        })
        .collect();
    found.sort();
    found.pop().map(|(_, path)| path)
}

/// A writing stage; its checksum check starts once the event loop runs. With a
/// checkout, it also says whether the image is behind it and offers a rebuild.
fn writing_with(image: Image, targets: Vec<Target>, repo: Option<&Path>) -> App {
    let checksum = match image::published_checksum(&image.path) {
        None => Checksum::Unpublished,
        Some(_) => Checksum::Checking { done: 0 },
    };
    // A downloaded release isn't built from this checkout, so it is never
    // "behind" or "up to date with" it.
    let downloaded = download::cache_dir().is_some_and(|dir| image.path.starts_with(dir));
    let checkout = match repo {
        _ if downloaded => Checkout::None,
        None => Checkout::None,
        Some(repo) if make::needs_build(repo, Some(&image.path)) => Checkout::Changed,
        Some(_) => Checkout::Current,
    };
    let mut app = App::new(image, checksum, targets);
    app.checkout = checkout;
    app
}

fn run(mut stage: Stage, repo: Option<PathBuf>, host: HostKind) -> std::io::Result<()> {
    let (tx, rx) = mpsc::channel();
    let cancel = Arc::new(AtomicBool::new(false));
    let mut build: Option<Arc<Mutex<Child>>> = None;
    let mut fetching = Arc::new(AtomicBool::new(false));
    match (&mut stage, &repo) {
        (Stage::Downloading(_), _) => fetching = start_download(&tx),
        (Stage::Building(view), Some(repo)) => build = start_build(repo, &tx, view),
        (Stage::Writing(app), _) => start_checksum(app, &tx),
        _ => {}
    }

    let colours = Colours::detect();
    let result = ratatui::run(|terminal| {
        loop {
            terminal.draw(|frame| {
                draw(frame, &mut stage);
                screen::fit(frame.buffer_mut(), colours);
            })?;
            if let Stage::Writing(app) = &stage
                && app.quit
            {
                return Ok(());
            }

            for event in rx.try_iter() {
                match (&mut stage, event) {
                    (Stage::Downloading(view), Event::Download(step)) => view.step(step),
                    (Stage::Downloading(view), Event::Downloaded(Err(why))) => view.failed(why),
                    (Stage::Downloading(view), Event::Downloaded(Ok(path))) => {
                        match writing(&path, &host, repo.as_deref()) {
                            Ok(app) => {
                                start_checksum(&app, &tx);
                                stage = Stage::Writing(Box::new(app));
                            }
                            Err(why) => view.failed(why),
                        }
                    }
                    (Stage::Building(view), Event::BuildLine(line)) => view.line(&line),
                    (Stage::Building(view), Event::Built(Err(why))) => {
                        build = None;
                        view.failed(why);
                    }
                    (Stage::Building(view), Event::Built(Ok(path))) => {
                        build = None;
                        match writing(&path, &host, repo.as_deref()) {
                            Ok(app) => {
                                start_checksum(&app, &tx);
                                stage = Stage::Writing(Box::new(app));
                            }
                            Err(why) => view.failed(why),
                        }
                    }
                    (Stage::Writing(app), event) => apply(app, event),
                    _ => {}
                }
            }

            if !event::poll(Duration::from_millis(100))? {
                continue;
            }
            let Term::Key(key) = event::read()? else { continue };
            if key.kind != KeyEventKind::Press {
                continue;
            }
            match &mut stage {
                Stage::Downloading(view) => match view.key(key) {
                    Some(DownloadRequest::Stop) => fetching.store(true, Ordering::Relaxed),
                    Some(DownloadRequest::Retry) => fetching = start_download(&tx),
                    Some(DownloadRequest::Quit) => return Ok(()),
                    None => {}
                },
                Stage::Writing(app) => match (app.key(key), &repo) {
                    (Some(Request::Rebuild), Some(repo)) => {
                        let mut view = BuildView::new(repo.join("target/live/build.log"));
                        build = start_build(repo, &tx, &mut view);
                        stage = Stage::Building(view);
                    }
                    (Some(request), _) => carry_out(request, app, &host, &cancel, &tx),
                    (None, _) => {}
                },
                Stage::Building(view) => match view.key(key) {
                    Some(BuildRequest::Stop) => {
                        stop_build(build.take());
                        view.failed("Stopped. Everything built so far is kept.".into());
                    }
                    Some(BuildRequest::Retry) => {
                        if let Some(repo) = &repo {
                            build = start_build(repo, &tx, view);
                        }
                    }
                    Some(BuildRequest::Quit) => return Ok(()),
                    None => {}
                },
            }
        }
    });
    // Never leave a build or download running behind a closed window.
    stop_build(build);
    fetching.store(true, Ordering::Relaxed);
    result
}

fn draw(frame: &mut ratatui::Frame, stage: &mut Stage) {
    match stage {
        Stage::Writing(app) => app.render(frame),
        Stage::Downloading(view) => view.render(frame),
        Stage::Building(view) => view.render(frame),
    }
}

/// Get the latest release's ISO into the cache in the background. Returns the
/// flag that stops it.
fn start_download(tx: &Sender<Event>) -> Arc<AtomicBool> {
    let stop = Arc::new(AtomicBool::new(false));
    let (flag, tx) = (Arc::clone(&stop), tx.clone());
    thread::spawn(move || {
        let result = match download::cache_dir() {
            Some(dir) => download::fetch(&dir, &flag, |step| {
                let _ = tx.send(Event::Download(step));
            }),
            None => Err("HOME is not set, so there is nowhere to keep the download".into()),
        };
        let _ = tx.send(Event::Downloaded(result));
    });
    stop
}

fn start_checksum(app: &App, tx: &Sender<Event>) {
    if let Some((expected, from)) = image::published_checksum(&app.image.path) {
        check_image(app.image.path.clone(), expected, from, tx.clone());
    }
}

/// Run `borea-build iso` from the checkout, streaming its output to the screen and
/// to `target/live/build.log`.
/// Start the ISO build; if it cannot start, say why on the build screen.
fn start_build(repo: &Path, tx: &Sender<Event>, view: &mut BuildView) -> Option<Arc<Mutex<Child>>> {
    let Some(cargo) = cargo() else {
        view.failed(
            "Cargo was not found. Install Rust with rustup (https://rustup.rs), then press Retry."
                .into(),
        );
        return None;
    };
    match spawn_build(repo, &cargo, tx) {
        Ok(child) => Some(child),
        Err(error) => {
            view.failed(format!("The build could not start: {error}"));
            None
        }
    }
}

/// Cargo: as `cargo` was run (`CARGO`), on PATH, or where rustup puts it. An app
/// launcher's PATH often lacks ~/.cargo/bin even when a terminal's has it.
fn cargo() -> Option<PathBuf> {
    let explicit = std::env::var_os("CARGO").map(PathBuf::from);
    let on_path = std::env::var_os("PATH")
        .into_iter()
        .flat_map(|path| std::env::split_paths(&path).collect::<Vec<_>>())
        .map(|dir| dir.join("cargo"));
    let rustup = std::env::var_os("CARGO_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".cargo")))
        .map(|home| home.join("bin/cargo"));
    explicit.into_iter().chain(on_path).chain(rustup).find(|p| p.is_file())
}

fn spawn_build(
    repo: &Path,
    cargo: &Path,
    tx: &Sender<Event>,
) -> std::io::Result<Arc<Mutex<Child>>> {
    // The ISO builder checks for cargo on PATH too, so give it the one found here.
    let mut path = cargo.parent().map(Path::to_path_buf).into_iter().collect::<Vec<_>>();
    if let Some(existing) = std::env::var_os("PATH") {
        path.extend(std::env::split_paths(&existing));
    }
    let path = std::env::join_paths(path).map_err(std::io::Error::other)?;
    let log_path = repo.join("target/live/build.log");
    std::fs::create_dir_all(repo.join("target/live"))?;
    // Append: a refused second build must not wipe the log of the one running.
    let mut file = std::fs::OpenOptions::new().create(true).append(true).open(&log_path)?;
    writeln!(file, "\n── borea-flash --make ──")?;
    let log = Arc::new(Mutex::new(file));
    let mut child = Command::new(cargo)
        .args(["run", "--release", "--quiet", "--manifest-path"])
        .arg(repo.join("Cargo.toml"))
        .args(["-p", "borea-build", "--", "iso"])
        .current_dir(repo)
        .env("PATH", path)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        // Its own process group, so stopping it stops make and every compiler too.
        .process_group(0)
        .spawn()?;
    let pipes: [Box<dyn std::io::Read + Send>; 2] = [
        Box::new(child.stdout.take().expect("piped")),
        Box::new(child.stderr.take().expect("piped")),
    ];
    for pipe in pipes {
        let (tx, log) = (tx.clone(), Arc::clone(&log));
        thread::spawn(move || {
            for line in BufReader::new(pipe).lines().map_while(Result::ok) {
                if let Ok(mut log) = log.lock() {
                    let _ = writeln!(log, "{line}");
                }
                let _ = tx.send(Event::BuildLine(line));
            }
        });
    }
    let child = Arc::new(Mutex::new(child));
    let (watched, tx, repo) = (Arc::clone(&child), tx.clone(), repo.to_path_buf());
    thread::spawn(move || {
        let status = loop {
            if let Ok(mut child) = watched.lock()
                && let Ok(Some(status)) = child.try_wait()
            {
                break status;
            }
            thread::sleep(Duration::from_millis(250));
        };
        let result = if status.success() {
            make::newest_image(&repo)
                .ok_or_else(|| "the build finished but left no image".to_owned())
        } else {
            let log = repo.join("target/live/build.log");
            Err(make::log_tail(&log, 16 << 20)
                .and_then(|text| make::failure(&text))
                .unwrap_or_else(|| format!("the build failed ({status}); see the log for details")))
        };
        let _ = tx.send(Event::Built(result));
    });
    Ok(child)
}

fn stop_build(build: Option<Arc<Mutex<Child>>>) {
    if let Some(child) = build
        && let Ok(child) = child.lock()
    {
        let group = rustix::process::Pid::from_child(&child);
        let _ = rustix::process::kill_process_group(group, rustix::process::Signal::INT);
    }
}

/// Download the latest release's ready-made borea-rataTUI and let it install
/// itself: the commands and the launcher, as `--install` does from a build.
fn update() -> Result<(), String> {
    let dir = download::cache_dir().ok_or("HOME is not set")?.join("app");
    say!("Downloading the latest borea-rataTUI…");
    let app = download::fetch_app(&dir)?;
    let status = Command::new(&app).arg("--install").status().map_err(|e| e.to_string())?;
    let _ = std::fs::remove_file(&app);
    status.success().then_some(()).ok_or_else(|| format!("installing it failed ({status})"))
}

/// Launch the installed program directly, so its TUI appears before any build.
fn install_launcher(helper: &Path) -> Result<(), String> {
    let entry = install::desktop_entry(helper);
    let home = std::env::var_os("HOME").map(PathBuf::from).ok_or("HOME is not set")?;
    let apps = home.join(".local/share/applications");
    let place = apps.join("borea-ratatui.desktop");
    std::fs::create_dir_all(&apps).map_err(|e| e.to_string())?;
    // The launcher used to be borea-live-maker.desktop; one entry is enough.
    let _ = std::fs::remove_file(apps.join("borea-live-maker.desktop"));
    std::fs::write(&place, &entry).map_err(|e| format!("{}: {e}", place.display()))?;
    say!("Added {}", place.display());
    say!("Open \"borea-rataTUI\" from your app launcher.");
    Ok(())
}

/// Copy this program to ~/.local/bin (or $XDG_BIN_HOME), so `borea-flash` runs
/// from any terminal. A copy, not a link: it keeps working after `cargo clean`.
fn install_command() -> Result<PathBuf, String> {
    let home = std::env::var_os("HOME").map(PathBuf::from).ok_or("HOME is not set")?;
    let dir = std::env::var_os("XDG_BIN_HOME")
        .map(PathBuf::from)
        .filter(|d| d.is_absolute())
        .unwrap_or_else(|| home.join(".local/bin"));
    let this = std::env::current_exe().map_err(|e| e.to_string())?;
    install::commands(&this, &dir).map_err(|e| e.to_string())?;
    say!(
        "Installed {} (also as {})",
        dir.join(install::NAMES[0]).display(),
        install::NAMES[1..].join(" and ")
    );
    let on_path =
        std::env::var_os("PATH").is_some_and(|path| std::env::split_paths(&path).any(|p| p == dir));
    if on_path {
        say!("Run `borea-rataTUI` to prepare the ISO and write a USB stick.");
    } else {
        say!(
            "{} is not on your PATH. Add this line to your shell's startup file:\n  \
             export PATH=\"{}:$PATH\"",
            dir.display(),
            dir.display()
        );
    }
    Ok(dir.join(install::NAMES[0]))
}

fn apply(app: &mut App, event: Event) {
    match event {
        Event::Checksum(state) => app.checksum = state,
        Event::Targets(targets) => app.set_targets(targets),
        Event::Progress(progress) => app.progress(progress),
        Event::Finished(result) => app.finished(result),
        Event::Message(message) => app.message = Some(message),
        Event::BuildLine(_) | Event::Built(_) | Event::Download(_) | Event::Downloaded(_) => {}
    }
}

fn carry_out(
    request: Request,
    app: &App,
    host: &HostKind,
    cancel: &Arc<AtomicBool>,
    tx: &Sender<Event>,
) {
    let (image, host, tx) = (app.image.clone(), host.clone(), tx.clone());
    match request {
        Request::Rescan => rescan(&image, &host, &tx),
        Request::Stop => cancel.store(true, Ordering::Relaxed),
        // The main loop switches to building; without a checkout there is nothing to do.
        Request::Rebuild => {}
        Request::Unmount(disk) => {
            thread::spawn(move || {
                if let Err(error) = device::unmount(&disk) {
                    let _ = tx.send(Event::Message(error.to_string()));
                }
                rescan(&image, &host, &tx);
            });
        }
        Request::Eject(name) => {
            thread::spawn(move || {
                let message = match device::eject(&name) {
                    Ok(()) => format!("✓ {name} is safe to remove."),
                    Err(error) => error.to_string(),
                };
                let _ = tx.send(Event::Message(message));
            });
        }
        Request::Flash(name) => {
            cancel.store(false, Ordering::Relaxed);
            let cancel = Arc::clone(cancel);
            thread::spawn(move || {
                let result = flash(&image, &name, &host, &cancel, &tx).map_err(|e| e.to_string());
                let _ = tx.send(Event::Finished(result));
            });
        }
    }
}

fn rescan(image: &Image, host: &HostKind, tx: &Sender<Event>) {
    let _ =
        tx.send(Event::Targets(target::discover(Path::new("/"), &image.path, image.size, host)));
}

/// Re-check the stick, open it exclusively, write, and verify.
fn flash(
    image: &Image,
    name: &str,
    host: &HostKind,
    cancel: &AtomicBool,
    tx: &Sender<Event>,
) -> Result<Outcome, FlashError> {
    let now = target::discover(Path::new("/"), &image.path, image.size, host);
    match now.iter().find(|t| t.disk.name == name).map(|t| &t.verdict) {
        Some(Verdict::Ready) => {}
        Some(verdict) => return Err(FlashError::Changed(name.into(), verdict.to_string())),
        None => return Err(FlashError::Changed(name.into(), "it was removed".into())),
    }
    let mut source = std::fs::File::open(&image.path).map_err(FlashError::Read)?;
    let mut stick = device::open(name)?;
    write::flash(&mut source, image.size, &mut stick, cancel, |progress| {
        let _ = tx.send(Event::Progress(progress));
    })
}

fn check_image(path: PathBuf, expected: String, from: PathBuf, tx: Sender<Event>) {
    thread::spawn(move || {
        let mut last = 0;
        let actual = image::sha256(&path, |done| {
            // Report every 64 MiB; the screen redraws ten times a second anyway.
            if done - last >= 64 << 20 {
                last = done;
                let _ = tx.send(Event::Checksum(Checksum::Checking { done }));
            }
        });
        let state = match actual {
            Ok(actual) if actual == expected => Checksum::Matches(from),
            Ok(_) => Checksum::Mismatch(from),
            Err(error) => Checksum::Failed(error.to_string()),
        };
        let _ = tx.send(Event::Checksum(state));
    });
}

#[cfg(test)]
mod cli_tests {
    use super::{arguments, take_wait_flag};

    #[test]
    fn the_launcher_flag_keeps_the_helpers_default() {
        let mut args = vec!["--wait-on-error".to_owned()];
        assert!(take_wait_flag(&mut args));
        assert_eq!(arguments(args, true), ["--get"]);
        let mut plain = vec!["image.iso".to_owned()];
        assert!(!take_wait_flag(&mut plain));
        assert_eq!(plain, ["image.iso"]);
    }

    #[test]
    fn helper_prepares_by_default_but_respects_explicit_actions() {
        assert_eq!(arguments(vec![], true), ["--get"]);
        for action in ["--list", "--help", "--install", "image.iso", "--build"] {
            assert_eq!(arguments(vec![action.into()], true), [action]);
        }
        assert!(arguments(vec![], false).is_empty());
    }
}
