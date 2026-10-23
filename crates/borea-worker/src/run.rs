//! Running the native tools: fixed paths and argument lists, a clean
//! environment, output to the install log, and a short tail kept for errors.

use std::collections::VecDeque;
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::thread;

pub const LOG: &str = "/run/borea/install.log";
const PATH: &str = "/usr/local/sbin:/usr/local/bin:/usr/bin:/usr/sbin:/bin:/sbin";
/// Lines of a failed command's output quoted in its error.
const TAIL: usize = 15;

pub struct Log {
    file: Arc<Mutex<File>>,
    verbose: bool,
    step: usize,
}

impl Log {
    pub fn open(verbose: bool) -> Result<Log, String> {
        fs::create_dir_all("/run/borea").map_err(|e| e.to_string())?;
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(LOG)
            .map_err(|e| format!("{LOG}: {e}"))?;
        Ok(Log { file: Arc::new(Mutex::new(file)), verbose, step: 0 })
    }

    /// A step of the install, numbered, on the terminal and in the log.
    pub fn step(&mut self, total: usize, what: &str) {
        self.step += 1;
        let line = format!("[{}/{total}] {what}", self.step);
        println!("{line}");
        self.write(&format!("\n== {line}"));
    }

    pub fn note(&self, what: &str) {
        println!("      {what}");
        self.write(what);
    }

    pub fn write(&self, line: &str) {
        if let Ok(mut file) = self.file.lock() {
            let _ = writeln!(file, "{line}");
        }
    }

    /// Run `program` with `args`, feeding `input` on stdin. Its output goes to
    /// the log (and the terminal with --verbose); on failure the error quotes
    /// its last lines.
    pub fn run(
        &self,
        program: &str,
        args: &[&str],
        input: Option<&[u8]>,
    ) -> Result<String, String> {
        // Arguments are logged, never the input: that is where secrets go.
        self.write(&format!("$ {program} {}", args.join(" ")));
        let mut child = Command::new(program)
            .args(args)
            .env_clear()
            .env("PATH", PATH)
            .env("LANG", "C.UTF-8")
            .env("TERM", "dumb")
            .stdin(if input.is_some() { Stdio::piped() } else { Stdio::null() })
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("{program}: {e}"))?;
        if let (Some(input), Some(mut stdin)) = (input, child.stdin.take()) {
            stdin.write_all(input).map_err(|e| format!("{program}: {e}"))?;
        }
        let tail = Arc::new(Mutex::new(VecDeque::new()));
        let stdout = Arc::new(Mutex::new(String::new()));
        let readers: Vec<_> = [
            (Box::new(child.stdout.take().expect("piped")) as Box<dyn std::io::Read + Send>, true),
            (Box::new(child.stderr.take().expect("piped")), false),
        ]
        .into_iter()
        .map(|(pipe, is_stdout)| {
            let (file, tail, stdout, verbose) =
                (Arc::clone(&self.file), Arc::clone(&tail), Arc::clone(&stdout), self.verbose);
            thread::spawn(move || {
                for line in BufReader::new(pipe).lines().map_while(Result::ok) {
                    if let Ok(mut file) = file.lock() {
                        let _ = writeln!(file, "{line}");
                    }
                    if verbose {
                        println!("        {line}");
                    }
                    if is_stdout && let Ok(mut out) = stdout.lock() {
                        out.push_str(&line);
                        out.push('\n');
                    }
                    if let Ok(mut tail) = tail.lock() {
                        tail.push_back(line);
                        if tail.len() > TAIL {
                            tail.pop_front();
                        }
                    }
                }
            })
        })
        .collect();
        let status = child.wait().map_err(|e| format!("{program}: {e}"))?;
        for reader in readers {
            let _ = reader.join();
        }
        let output = stdout.lock().map(|s| s.clone()).unwrap_or_default();
        if status.success() {
            return Ok(output);
        }
        let tail: Vec<String> =
            tail.lock().map(|t| t.iter().cloned().collect()).unwrap_or_default();
        Err(format!(
            "{} {} failed ({status}):\n    {}\n  Full log: {LOG}",
            Path::new(program).file_name().unwrap_or_default().to_string_lossy(),
            args.first().copied().unwrap_or_default(),
            tail.join("\n    ")
        ))
    }
}
