//! The app's side: a pool of geometry-host processes that regenerates parts
//! ([`oc_asm::parallel::Runner`]). Each host runs one part at a time; a host that crashes,
//! hangs past the timeout or breaks the protocol is killed and started again, and only the
//! part it was regenerating fails, with an error on the part. Hosts are started on the first
//! batch and kept for the next ones. On Windows (G3d) hosts answer on a pipe of their own
//! (their standard output goes to `NUL`) and are in a job object that ends them with the
//! app, even when the app is killed ([`crate::win`]).
use std::io::{BufReader, BufWriter, Read, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError};
use std::sync::Arc;
use std::time::{Duration, Instant};

use oc_asm::parallel::{worker_count, PartJob, PartOut, Runner};
use oc_naming::Rules;

use crate::proto::{self, decode_done, decode_failed, decode_hello, encode_job, read_frame, write_frame, Job};

/// How to start a host: a program, its arguments and extra environment variables.
#[derive(Clone, Debug)]
pub struct HostCommand {
    pub program: PathBuf,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
}

impl HostCommand {
    pub fn new(program: impl Into<PathBuf>) -> Self {
        HostCommand { program: program.into(), args: vec![], env: vec![] }
    }
    pub fn arg(mut self, a: &str) -> Self {
        self.args.push(a.into());
        self
    }
    pub fn env(mut self, k: &str, v: &str) -> Self {
        self.env.push((k.into(), v.into()));
        self
    }
}

/// What the pool has done (shared: the pool itself moves into an assembler).
#[derive(Debug, Default)]
pub struct HostStats {
    /// Host processes started (restarts included).
    pub started: AtomicUsize,
    /// Hosts that died while regenerating a part (crash, abort, exit, broken stream).
    pub died: AtomicUsize,
    /// Hosts killed for not answering within the timeout.
    pub timed_out: AtomicUsize,
    /// Hosts that could not be started.
    pub failed_to_start: AtomicUsize,
}

/// A part's error when its host died while regenerating it (🔍 wording).
pub const DIED: &str = "The geometry kernel stopped while rebuilding this part. The part may contain geometry the kernel cannot build; the rest of the assembly is unaffected.";

/// A part's error when its host did not answer in time (🔍 wording).
pub fn timed_out(after: Duration) -> String {
    format!("The rebuild of this part did not finish within {} s and was stopped.", after.as_secs())
}

struct Host {
    child: Child,
    input: BufWriter<ChildStdin>,
    frames: Receiver<Result<(u32, Vec<u8>), String>>,
}

impl Host {
    /// Starts a host (in `group`); it is ready once its Hello arrives (checked on the first
    /// call).
    fn spawn(cmd: &HostCommand, group: Option<&Group>) -> Result<Host, String> {
        let mut c = Command::new(&cmd.program);
        c.args(&cmd.args).stdin(Stdio::piped()).stderr(Stdio::null());
        // Replies on a pipe of their own (Windows), so what the kernel prints goes nowhere.
        c.stdout(if cfg!(windows) { Stdio::null() } else { Stdio::piped() });
        for (k, v) in &cmd.env {
            c.env(k, v);
        }
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            // No console window for a console-subsystem host started from the GUI app.
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            c.creation_flags(CREATE_NO_WINDOW);
        }
        let mut child = c.spawn().map_err(|e| format!("{}: {e}", cmd.program.display()))?;
        let mut input = BufWriter::new(child.stdin.take().ok_or("no host input")?);
        let output = match reply_pipe(&mut child, &mut input, group) {
            Ok(o) => o,
            Err(e) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(e);
            }
        };
        let mut output = BufReader::new(output);
        let (tx, frames) = channel();
        std::thread::Builder::new()
            .name("geometry-host-reader".into())
            .spawn(move || loop {
                let f = match read_frame(&mut output) {
                    Ok(Some(f)) => Ok(f),
                    Ok(None) => Err("the geometry host exited".to_string()),
                    Err(e) => Err(e.to_string()),
                };
                let end = f.is_err();
                if tx.send(f).is_err() || end {
                    break;
                }
            })
            .map_err(|e| e.to_string())?;
        Ok(Host { child, input, frames })
    }

    fn kill(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// The host's reply stream. Windows: the host goes in the job, then a pipe is made, its write
/// end duplicated into the host and named in the first frame; the app keeps the read end
/// only, so the stream ends when the host does.
#[cfg(windows)]
fn reply_pipe(child: &mut Child, input: &mut BufWriter<ChildStdin>, group: Option<&Group>) -> Result<Box<dyn Read + Send>, String> {
    use std::os::windows::io::AsRawHandle;
    if let Some(g) = group {
        g.add(child.as_raw_handle());
    }
    let (read, write) = crate::win::pipe().map_err(|e| e.to_string())?;
    let there = crate::win::give(&write, child.as_raw_handle()).map_err(|e| e.to_string())?;
    drop(write);
    write_frame(input, proto::OUTPUT, &there.to_le_bytes()).map_err(|e| e.to_string())?;
    Ok(Box::new(read))
}

#[cfg(not(windows))]
fn reply_pipe(child: &mut Child, _: &mut BufWriter<ChildStdin>, _: Option<&Group>) -> Result<Box<dyn Read + Send>, String> {
    Ok(Box::new(child.stdout.take().ok_or("no host output")?))
}

/// What ends the hosts with the app: on Windows a job object that kills on close.
#[cfg(windows)]
use crate::win::Job as Group;
#[cfg(not(windows))]
struct Group;

fn new_group() -> Option<Group> {
    #[cfg(windows)]
    return Group::new();
    #[cfg(not(windows))]
    None
}

/// One host's slot: its process (started when first needed) and whether it said Hello.
struct Slot {
    host: Option<Host>,
    ready: bool,
}

/// Geometry-host processes regenerating parts, `size` at once.
pub struct Pool {
    cmd: HostCommand,
    size: usize,
    /// How long a part may take before its host is stopped.
    pub timeout: Duration,
    /// How long a new host may take to say Hello.
    pub start_timeout: Duration,
    /// Hosts unused for this long are stopped when the app is idle ([`Runner::idle`]).
    pub keep: Duration,
    last_used: Instant,
    slots: Vec<Slot>,
    stats: Arc<HostStats>,
    /// Every host is put in it (G3d); `None` where there is none or Windows refused.
    group: Option<Group>,
}

/// What a call to a host gave.
enum Answer {
    Done(Box<PartOut>),
    /// The job could not run (an unreadable document): regenerate it in the app.
    Failed(String),
    /// The host died or broke the protocol.
    Died,
    TimedOut,
}

impl Pool {
    /// `size` hosts (0: half the logical processors, [`oc_asm::parallel::default_workers`]).
    pub fn new(cmd: HostCommand, size: usize) -> Pool {
        Pool {
            cmd,
            size: worker_count(size, usize::MAX),
            timeout: Duration::from_secs(600),
            start_timeout: Duration::from_secs(60),
            keep: Duration::from_secs(300),
            last_used: Instant::now(),
            slots: vec![],
            stats: Arc::new(HostStats::default()),
            group: new_group(),
        }
    }

    pub fn stats(&self) -> Arc<HostStats> {
        self.stats.clone()
    }

    /// The process ids of the hosts running now.
    pub fn processes(&self) -> Vec<u32> {
        self.slots.iter().filter_map(|s| s.host.as_ref().map(|h| h.child.id())).collect()
    }

    /// Hosts running now.
    pub fn running(&self) -> usize {
        self.slots.iter().filter(|s| s.host.is_some()).count()
    }

    /// Starts the hosts a batch of `n` parts needs (without waiting for them).
    fn start(&mut self, n: usize) {
        while self.slots.len() < n {
            self.slots.push(Slot { host: None, ready: false });
        }
        for s in self.slots.iter_mut().take(n) {
            if s.host.is_none() {
                start(&self.cmd, &self.stats, self.group.as_ref(), s);
            }
        }
    }
}

fn start(cmd: &HostCommand, stats: &HostStats, group: Option<&Group>, s: &mut Slot) {
    s.ready = false;
    match Host::spawn(cmd, group) {
        Ok(h) => {
            stats.started.fetch_add(1, Ordering::Relaxed);
            s.host = Some(h);
        }
        Err(_) => {
            stats.failed_to_start.fetch_add(1, Ordering::Relaxed);
            s.host = None;
        }
    }
}

/// Waits for a new host's Hello; false (and the host stopped) if it does not come.
fn ready(s: &mut Slot, within: Duration) -> bool {
    if s.ready {
        return true;
    }
    let Some(h) = s.host.as_mut() else { return false };
    let ok = matches!(h.frames.recv_timeout(within), Ok(Ok((proto::HELLO, p))) if decode_hello(&p).is_ok_and(|(v, _)| v == proto::PROTOCOL));
    if !ok {
        h.kill();
        s.host = None;
    }
    s.ready = ok;
    ok
}

fn call(h: &mut Host, job: &Job, timeout: Duration) -> Answer {
    if write_frame(&mut h.input, proto::JOB, &encode_job(job)).is_err() {
        return Answer::Died;
    }
    match h.frames.recv_timeout(timeout) {
        Ok(Ok((proto::DONE, p))) => match decode_done(&p) {
            Ok((id, out)) if id == job.id => Answer::Done(Box::new(out)),
            _ => Answer::Died,
        },
        Ok(Ok((proto::FAILED, p))) => match decode_failed(&p) {
            Ok((id, m)) if id == job.id => Answer::Failed(m),
            _ => Answer::Died,
        },
        Ok(_) | Err(RecvTimeoutError::Disconnected) => Answer::Died,
        Err(RecvTimeoutError::Timeout) => Answer::TimedOut,
    }
}

impl Runner for Pool {
    fn width(&self, jobs: usize) -> usize {
        self.size.min(jobs).max(1)
    }

    fn warm(&mut self) {
        self.start(self.size);
        self.last_used = Instant::now();
    }

    fn idle(&mut self) {
        if self.last_used.elapsed() < self.keep {
            return;
        }
        for s in &mut self.slots {
            if let Some(mut h) = s.host.take() {
                h.kill();
            }
            s.ready = false;
        }
    }

    fn run(&mut self, rules: Rules, jobs: &[PartJob]) -> Vec<Result<PartOut, String>> {
        let width = self.width(jobs.len());
        self.start(width);
        self.last_used = Instant::now();
        let next = AtomicUsize::new(0);
        let (cmd, stats, group, timeout, start_timeout) = (&self.cmd, &*self.stats, self.group.as_ref(), self.timeout, self.start_timeout);
        let mut results: Vec<Option<Result<PartOut, String>>> = (0..jobs.len()).map(|_| None).collect();
        std::thread::scope(|sc| {
            let handles: Vec<_> = self
                .slots
                .iter_mut()
                .take(width)
                .map(|slot| {
                    let next = &next;
                    sc.spawn(move || {
                        let mut done = vec![];
                        loop {
                            let j = next.fetch_add(1, Ordering::Relaxed);
                            let Some(job) = jobs.get(j) else { break };
                            if slot.host.is_none() {
                                start(cmd, stats, group, slot);
                            }
                            if !ready(slot, start_timeout) {
                                // No host: the app regenerates the part itself.
                                done.push((j, Err("no geometry host could be started".to_string())));
                                continue;
                            }
                            let doc = match oc_doc::save(&job.doc) {
                                Ok(d) => d,
                                Err(e) => {
                                    done.push((j, Err(e.to_string())));
                                    continue;
                                }
                            };
                            let host = slot.host.as_mut().expect("ready");
                            let t = Instant::now();
                            let r = match call(host, &Job { id: j as u64, quality: job.quality, rules, doc }, timeout) {
                                Answer::Done(out) => Ok(*out),
                                Answer::Failed(m) => Err(m),
                                Answer::Died => {
                                    stats.died.fetch_add(1, Ordering::Relaxed);
                                    host.kill();
                                    slot.host = None;
                                    Ok(PartOut::failed(DIED))
                                }
                                Answer::TimedOut => {
                                    stats.timed_out.fetch_add(1, Ordering::Relaxed);
                                    host.kill();
                                    slot.host = None;
                                    Ok(PartOut::failed(timed_out(t.elapsed())))
                                }
                            };
                            done.push((j, r));
                        }
                        done
                    })
                })
                .collect();
            for h in handles {
                for (j, r) in h.join().unwrap_or_default() {
                    results[j] = Some(r);
                }
            }
        });
        // A host that died is started again when its slot takes its next part (in this batch
        // or the next).
        self.last_used = Instant::now();
        results.into_iter().map(|r| r.unwrap_or_else(|| Err("the geometry host pool stopped".into()))).collect()
    }
}

impl Drop for Pool {
    fn drop(&mut self) {
        for s in &mut self.slots {
            if let Some(h) = s.host.as_mut() {
                // Closing its input ends a host's loop; kill in case it is busy.
                let _ = h.input.flush();
                h.kill();
            }
        }
    }
}
