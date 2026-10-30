use borea_flash::app::{App, Checkout, Checksum};
use borea_flash::download::{DownloadView, Step};
use borea_flash::image::Image;
use borea_flash::make::BuildView;
use borea_flash::target::{Target, Verdict};
use borea_flash::write::{Outcome, Phase, Progress};
use borea_hardware::{Disk, DiskTransport, DiskUsage};
use ratatui::{
    Terminal,
    backend::TestBackend,
    crossterm::event::{KeyCode, KeyEvent},
};

fn show(name: &str, w: u16, h: u16, mut draw: impl FnMut(&mut ratatui::Frame)) {
    let mut t = Terminal::new(TestBackend::new(w, h)).unwrap();
    t.draw(|f| draw(f)).unwrap();
    let b = t.backend().buffer().clone();
    let rows: Vec<String> =
        b.content.chunks(w as usize).map(|row| row.iter().map(|c| c.symbol()).collect()).collect();
    if std::env::var_os("DUMP").is_some() {
        println!("==== {name} {w}x{h}");
        for row in &rows {
            println!("|{row}|");
        }
    }
    let text = rows.join("\n");
    assert!(text.contains("Borea"), "{name} {w}x{h}: no header\n{text}");
    for hint in ["q quit", "Esc stop", "Esc back"] {
        let whole = text.contains(hint);
        let cut = rows.iter().any(|r| r.trim_end().ends_with(&hint[..hint.len() - 1]));
        assert!(whole || !cut, "{name} {w}x{h}: \"{hint}\" cut off\n{text}");
    }
}
fn stick(n: &str, m: &str, v: Verdict) -> Target {
    Target {
        disk: Disk {
            name: n.into(),
            model: Some(m.into()),
            size_bytes: 32_000_000_000,
            removable: true,
            rotational: None,
            transport: DiskTransport::Usb,
            partitions: vec![],
            usage: DiskUsage::Free,
        },
        verdict: v,
    }
}

#[test]
fn zz_dump() {
    if std::env::var_os("DUMP").is_none() {
        return;
    }
    for (w, h) in [(80u16, 24u16), (60, 20)] {
        let mut d = DownloadView::new();
        show("download start", w, h, |f| d.render(f));
        d.step(Step::Receiving { done: 700_000_000, total: Some(2_100_000_000) });
        show("download progress", w, h, |f| d.render(f));
        d.failed("No Borea Live release has been published yet.".into());
        show("download failed", w, h, |f| d.render(f));
        let _ = d.key(KeyEvent::from(KeyCode::Char('x')));

        let mut bv = BuildView::new("/home/luke/borea-live/target/live/build.log".into());
        for l in [
            ">>> host-gcc-final 14.2.0 Building",
            "gcc -O2 -c foo.c -o foo.o with a really long command line that goes on and on and on and on",
            ">>> linux-pam 1.6.1 Installing to target",
        ] {
            bv.line(l);
        }
        show("build", w, h, |f| bv.render(f));
        bv.failed("linux-pam: ERROR: Program 'byacc' not found or not executable".into());
        show("build failed", w, h, |f| bv.render(f));

        let image = Image {
            path: "/home/luke/.cache/borea/borea-live-x86_64.iso".into(),
            size: 2_100_000_000,
            label: Some("BOREA_LIVE".into()),
            hybrid: true,
        };
        let mut app = App::new(
            image.clone(),
            Checksum::Matches("/home/luke/.cache/borea/borea-live-x86_64.iso.sha256".into()),
            vec![
                stick("sdb", "SanDisk Ultra", Verdict::Ready),
                stick(
                    "sdc",
                    "Kingston DataTraveler",
                    Verdict::Mounted(vec!["/run/media/luke/KINGSTON".into()]),
                ),
                stick(
                    "sda",
                    "Old Installer",
                    Verdict::Refused("this live session may be running from it".into()),
                ),
            ],
        );
        app.checkout = Checkout::Changed;
        show("choose", w, h, |f| app.render(f));
        let mut app2 = App::new(image.clone(), Checksum::Unpublished, vec![]);
        show("choose empty", w, h, |f| app2.render(f));
        app.key(KeyEvent::from(KeyCode::Enter));
        app.key(KeyEvent::from(KeyCode::Char('s')));
        show("confirm", w, h, |f| app.render(f));
        app.progress(Progress { phase: Phase::Writing, done: 1_300_000_000, total: 2_100_000_000 });
        show("writing", w, h, |f| app.render(f));
        app.progress(Progress { phase: Phase::Verifying, done: 500_000_000, total: 2_100_000_000 });
        show("verifying", w, h, |f| app.render(f));
        app.key(KeyEvent::from(KeyCode::Esc));
        show("stop asked", w, h, |f| app.render(f));
        app.finished(Ok(Outcome::Verified));
        show("done", w, h, |f| app.render(f));
        app.finished(Err("writing the stick failed: Input/output error (os error 5)".into()));
        show("error", w, h, |f| app.render(f));
    }
}
