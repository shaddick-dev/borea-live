//! Handing off to another distribution's own installer (Omarchy first), through
//! borea-installd's `handoff.review` and `handoff.start` (crates/borea-handoff).
//!
//! Requests run off the UI thread. Nothing is preselected: the person picks the
//! disk, and whether Omarchy takes all of it or space beside Windows; Borea asks
//! the service for a review and an approval digest, and only that approval can
//! start the hand-off. Tests drive it with a fake service.

use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::Duration;

use eframe::egui;
use serde::Deserialize;
use serde_json::{Value, json};

pub const GIB: u64 = 1 << 30;
const STARTING: &str = "Starting…";

/// A disk as `disks.list` reports it. `id` is kept as sent, to send back unchanged.
#[derive(Debug, Clone, Deserialize)]
pub struct Disk {
    pub id: Value,
    #[serde(default)]
    pub readonly: bool,
    #[serde(default)]
    pub in_use: bool,
    #[serde(default)]
    pub live: bool,
    #[serde(default)]
    pub parts: Vec<Part>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Part {
    pub path: String,
    pub number: u32,
    pub uuid: String,
    #[serde(default)]
    pub fs: String,
    #[serde(default)]
    pub max_linux_size: Option<u64>,
}

impl Disk {
    pub fn path(&self) -> &str {
        self.id["path"].as_str().unwrap_or_default()
    }
    pub fn size(&self) -> u64 {
        self.id["size"].as_u64().unwrap_or_default()
    }
    /// "Samsung 980 · 512 GB", or the device when there is no model.
    pub fn name(&self) -> String {
        let model = self.id["model"].as_str().unwrap_or_default().trim();
        let model = if model.is_empty() { self.path() } else { model };
        format!("{model} · {}", gb(self.size()))
    }
    /// Disks Borea may offer at all: not the live stick, not mounted, writable.
    pub fn usable(&self) -> bool {
        !self.live && !self.in_use && !self.readonly
    }
}

pub fn gb(bytes: u64) -> String {
    format!("{:.0} GB", bytes as f64 / 1e9)
}

/// Where the person chose to put the new system.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Where {
    Whole { disk: usize },
    Alongside { disk: usize, part: usize, size: u64 },
}

/// The service's review of the chosen hand-off.
#[derive(Debug, Clone, PartialEq)]
pub struct Reviewed {
    pub steps: Vec<String>,
    pub choose: String,
    pub approval: String,
    pub plan: Value,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Phase {
    /// Not asked for disks yet.
    Idle,
    Loading,
    /// Disks are listed; the person chooses.
    Choosing,
    Reviewing,
    Reviewed(Reviewed),
    Running {
        /// Stages already finished, in order.
        done: Vec<String>,
        stage: String,
        fraction: Option<f32>,
        lines: Vec<String>,
        choose: String,
    },
    /// Borea is restarting into the other installer.
    Finished {
        choose: String,
    },
    Failed(String),
}

/// Answers a JSON-RPC method with params: borea-installd, or a fake in tests.
pub type Service = Arc<dyn Fn(&str, &Value) -> Result<Value, String> + Send + Sync>;

enum Msg {
    Disks(Result<Vec<Disk>, String>),
    Review(Result<Value, String>),
    Start(Result<Value, String>),
    Events(Result<Value, String>),
}

pub struct HandoffLink {
    pub target: &'static str,
    pub phase: Phase,
    pub disks: Vec<Disk>,
    pub pick: Option<Where>,
    /// For alongside: the person agreed Windows is shrunk now.
    pub confirmed: bool,
    service: Service,
    tx: Sender<Msg>,
    rx: Receiver<Msg>,
    ctx: Option<egui::Context>,
}

impl HandoffLink {
    /// The real service: borea-installd over its socket.
    pub fn system(target: &'static str, ctx: egui::Context) -> Self {
        let service: Service = Arc::new(|method, params| {
            borea_protocol::call::<_, Value>(method, params, Duration::from_secs(120))
                .map_err(|e| e.to_string())
        });
        let mut link = Self::with_service(target, service);
        link.ctx = Some(ctx);
        link
    }

    pub fn with_service(target: &'static str, service: Service) -> Self {
        let (tx, rx) = mpsc::channel();
        HandoffLink {
            target,
            phase: Phase::Idle,
            disks: Vec::new(),
            pick: None,
            confirmed: false,
            service,
            tx,
            rx,
            ctx: None,
        }
    }

    fn ask(&self, method: &'static str, params: Value, wrap: fn(Result<Value, String>) -> Msg) {
        let (service, tx, ctx) = (Arc::clone(&self.service), self.tx.clone(), self.ctx.clone());
        let _ = thread::Builder::new().name("handoff".into()).spawn(move || {
            let _ = tx.send(wrap(service(method, &params)));
            if let Some(ctx) = ctx {
                ctx.request_repaint();
            }
        });
    }

    /// List the disks, once, when the step that needs them opens.
    pub fn load(&mut self) {
        if self.phase != Phase::Idle {
            return;
        }
        self.phase = Phase::Loading;
        let (service, tx, ctx) = (Arc::clone(&self.service), self.tx.clone(), self.ctx.clone());
        let _ = thread::Builder::new().name("handoff-disks".into()).spawn(move || {
            let disks = service("disks.list", &Value::Null).and_then(|facts| {
                serde_json::from_value::<Vec<Disk>>(facts["disks"].clone())
                    .map_err(|e| format!("The disk list could not be read: {e}"))
            });
            let _ = tx.send(Msg::Disks(disks));
            if let Some(ctx) = ctx {
                ctx.request_repaint();
            }
        });
    }

    /// Choose where it goes. Any change needs a new review.
    pub fn choose(&mut self, pick: Where) {
        if self.pick.as_ref() != Some(&pick) {
            self.pick = Some(pick);
            self.confirmed = false;
            if matches!(self.phase, Phase::Reviewing | Phase::Reviewed(_) | Phase::Failed(_)) {
                self.phase = Phase::Choosing;
            }
        }
    }

    pub fn confirm(&mut self, on: bool) {
        self.confirmed = on;
    }

    /// The hand-off plan for the current choice, as borea-handoff's `Handoff`.
    pub fn plan(&self) -> Option<Value> {
        let disk = match self.pick.as_ref()? {
            Where::Whole { disk } => json!({"kind":"whole","disk":self.disks.get(*disk)?.id}),
            Where::Alongside { disk, part, size } => {
                let d = self.disks.get(*disk)?;
                let p = d.parts.get(*part)?;
                json!({"kind":"alongside","disk":d.id,
                       "windows_part":{"path":p.path,"uuid":p.uuid,"number":p.number},
                       "linux_size":size})
            }
        };
        Some(json!({"version":1,"target":self.target,"disk":disk}))
    }

    pub fn review(&mut self) {
        if let Some(plan) = self.plan() {
            self.phase = Phase::Reviewing;
            self.ask("handoff.review", json!({ "handoff": plan }), Msg::Review);
        }
    }

    /// Ready for "Begin installation": reviewed, and for alongside, confirmed.
    pub fn ready(&self) -> bool {
        matches!(self.phase, Phase::Reviewed(_))
            && (self.confirmed || matches!(self.pick, Some(Where::Whole { .. })))
    }

    pub fn start(&mut self) {
        if !self.ready() {
            return;
        }
        let Phase::Reviewed(r) = &self.phase else { return };
        let params = json!({"handoff": r.plan, "approval": r.approval});
        self.phase = Phase::Running {
            done: Vec::new(),
            stage: STARTING.into(),
            fraction: None,
            lines: Vec::new(),
            choose: r.choose.clone(),
        };
        self.ask("handoff.start", params, Msg::Start);
    }

    /// Follow the hand-off's events until it finishes or fails.
    fn follow(&self, after: u64) {
        let (service, tx, ctx) = (Arc::clone(&self.service), self.tx.clone(), self.ctx.clone());
        let _ = thread::Builder::new().name("handoff-events".into()).spawn(move || {
            let mut cursor = after;
            loop {
                let batch = service("install.events", &json!({ "after": cursor }));
                let done =
                    batch.as_ref().map_or(true, |b| {
                        b["events"].as_array().into_iter().flatten().any(|e| {
                            matches!(e["event"].as_str(), Some("finished") | Some("failed"))
                        })
                    });
                if let Ok(b) = &batch {
                    cursor = b["cursor"].as_u64().unwrap_or(cursor);
                }
                let more = batch.as_ref().is_ok_and(|b| b["more"].as_bool() == Some(true));
                let _ = tx.send(Msg::Events(batch));
                if let Some(ctx) = &ctx {
                    ctx.request_repaint();
                }
                if done {
                    break;
                }
                if !more {
                    thread::sleep(Duration::from_millis(400));
                }
            }
        });
    }

    pub fn poll(&mut self) {
        while let Ok(msg) = self.rx.try_recv() {
            match msg {
                Msg::Disks(Ok(disks)) => {
                    self.disks = disks;
                    self.phase = Phase::Choosing;
                }
                Msg::Disks(Err(e)) => {
                    self.phase =
                        Phase::Failed(format!("The installer service is not available: {e}"))
                }
                // A review for a choice since changed is no longer this one's.
                Msg::Review(_) if self.phase != Phase::Reviewing => {}
                Msg::Review(Ok(v)) => {
                    let review = &v["review"];
                    let list = |key: &str| {
                        review[key]
                            .as_array()
                            .into_iter()
                            .flatten()
                            .filter_map(|s| s.as_str().map(str::to_owned))
                            .collect::<Vec<_>>()
                    };
                    match (v["approval"].as_str(), self.plan()) {
                        (Some(approval), Some(plan)) => {
                            self.phase = Phase::Reviewed(Reviewed {
                                steps: list("steps"),
                                choose: review["choose"].as_str().unwrap_or_default().to_owned(),
                                approval: approval.to_owned(),
                                plan,
                            })
                        }
                        _ => self.phase = Phase::Failed("The review came back incomplete.".into()),
                    }
                }
                Msg::Review(Err(e)) => self.phase = Phase::Failed(e),
                Msg::Start(Ok(_)) => self.follow(0),
                Msg::Start(Err(e)) => self.phase = Phase::Failed(e),
                Msg::Events(Err(e)) => {
                    self.phase =
                        Phase::Failed(format!("Lost contact with the installer service: {e}"))
                }
                Msg::Events(Ok(batch)) => self.events(&batch),
            }
        }
    }

    fn events(&mut self, batch: &Value) {
        for e in batch["events"].as_array().into_iter().flatten() {
            let Phase::Running { done, stage, fraction, lines, choose } = &mut self.phase else {
                return;
            };
            match e["event"].as_str() {
                Some("stage-started") => {
                    let next = e["title"].as_str().unwrap_or_default().to_owned();
                    let previous = std::mem::replace(stage, next);
                    if previous != STARTING {
                        done.push(previous);
                    }
                    *fraction = None;
                    // Each stage shows only its own details.
                    lines.clear();
                }
                Some("progress") => *fraction = e["fraction"].as_f64().map(|f| f as f32),
                Some("line") => {
                    lines.push(e["text"].as_str().unwrap_or_default().to_owned());
                    if lines.len() > 6 {
                        lines.remove(0);
                    }
                }
                Some("failed") => {
                    let why = e["fault"]["title"].as_str().unwrap_or("The hand-off failed.");
                    self.phase = Phase::Failed(why.to_owned());
                }
                Some("finished") => {
                    let choose = std::mem::take(choose);
                    self.phase = Phase::Finished { choose };
                }
                _ => {}
            }
        }
    }

    /// After a failure: list the disks again and choose again.
    pub fn retry(&mut self) {
        self.phase = Phase::Idle;
        self.pick = None;
        self.confirmed = false;
        self.load();
    }

    pub fn busy(&self) -> bool {
        matches!(self.phase, Phase::Running { .. } | Phase::Finished { .. })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    fn disk(path: &str, model: &str, size: u64, parts: Value) -> Value {
        json!({"id":{"logical_sector":512,"path":path,"serial":"S1","size":size,"model":model},
               "readonly":false,"in_use":false,"live":false,"removable":false,"transport":"nvme",
               "parts":parts})
    }

    fn wait(link: &mut HandoffLink, done: impl Fn(&Phase) -> bool) {
        for _ in 0..200 {
            link.poll();
            if done(&link.phase) {
                return;
            }
            thread::sleep(Duration::from_millis(10));
        }
        panic!("stuck at {:?}", link.phase);
    }

    #[test]
    fn plans_are_what_the_service_expects_and_only_the_approval_starts() {
        let calls = Arc::new(Mutex::new(Vec::<(String, Value)>::new()));
        let seen = Arc::clone(&calls);
        let service: Service = Arc::new(move |method, params| {
            seen.lock().unwrap().push((method.into(), params.clone()));
            Ok(match method {
                "disks.list" => {
                    json!({"uefi":true,"disks":[disk("/dev/nvme0n1","Samsung 980",512_000_000_000,json!([
                    {"path":"/dev/nvme0n1p3","number":3,"uuid":"W","start":1,"size":400,"fs":"ntfs","max_linux_size":200*GIB}]))]})
                }
                "handoff.review" => {
                    json!({"approval":"abc","review":{"steps":["Shrink Windows"],"choose":"Pick Free space install"}})
                }
                "handoff.start" => json!({"job_id":"abc"}),
                _ => {
                    json!({"events":[{"event":"stage-started","stage":0,"title":"Downloading Omarchy."},{"event":"finished"}],"cursor":2,"more":false})
                }
            })
        });
        let mut link = HandoffLink::with_service("omarchy", service);
        link.load();
        wait(&mut link, |p| *p == Phase::Choosing);
        assert!(!link.ready() && link.plan().is_none(), "nothing is preselected");
        link.choose(Where::Alongside { disk: 0, part: 0, size: 60 * GIB });
        let plan = link.plan().unwrap();
        assert_eq!(plan["disk"]["kind"], "alongside");
        assert_eq!(plan["disk"]["windows_part"]["path"], "/dev/nvme0n1p3");
        assert_eq!(plan["disk"]["linux_size"], 60 * GIB);
        assert_eq!(plan["disk"]["disk"]["serial"], "S1", "the disk identity goes back unchanged");
        link.review();
        wait(&mut link, |p| matches!(p, Phase::Reviewed(_)));
        assert!(!link.ready(), "shrinking Windows needs the person's confirmation");
        link.confirm(true);
        assert!(link.ready());
        link.start();
        wait(&mut link, |p| matches!(p, Phase::Finished { .. }));
        assert_eq!(link.phase, Phase::Finished { choose: "Pick Free space install".into() });
        let calls = calls.lock().unwrap();
        let start = calls.iter().find(|(m, _)| m == "handoff.start").unwrap();
        assert_eq!(start.1["approval"], "abc");
        assert_eq!(start.1["handoff"], plan);
    }

    #[test]
    fn a_new_choice_needs_a_new_review_and_failures_are_shown() {
        let service: Service = Arc::new(|method, _| match method {
            "disks.list" => {
                Ok(json!({"uefi":true,"disks":[disk("/dev/vda","",100_000_000_000,json!([]))]}))
            }
            "handoff.review" => Ok(json!({"approval":"x","review":{"steps":[],"choose":"c"}})),
            _ => Err("No space on the Borea stick".into()),
        });
        let mut link = HandoffLink::with_service("omarchy", service);
        link.load();
        wait(&mut link, |p| *p == Phase::Choosing);
        assert_eq!(link.disks[0].name(), "/dev/vda · 100 GB");
        link.choose(Where::Whole { disk: 0 });
        link.review();
        wait(&mut link, |p| matches!(p, Phase::Reviewed(_)));
        assert!(link.ready(), "a whole disk is confirmed by the other installer itself");
        link.choose(Where::Whole { disk: 0 });
        assert!(link.ready(), "the same choice keeps its review");
        link.start();
        wait(&mut link, |p| matches!(p, Phase::Failed(_)));
        assert_eq!(link.phase, Phase::Failed("No space on the Borea stick".into()));
    }
}
