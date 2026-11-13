//! P3 spike: the geometry host.
//!
//! The parent (the future engine/UI process) never links OCCT. It spawns this same binary
//! with `--child`; the child owns OCCT, builds and tessellates shapes, and writes meshes
//! into a shared-memory segment. Control messages are single lines on stdin/stdout.
//!
//! Measured:
//!   * transfer: from the child finishing the write to the parent holding its own copy of
//!     the mesh (what a GPU staging upload would read), for ~1M triangles; target < 20 ms;
//!   * crash recovery: the child dies inside a kernel call (access violation / abort) or
//!     hangs; the parent notices, restarts it and retries; target: the parent survives.
//!
//! Run: cargo run --release -p p3-host
use shared_memory::{Shmem, ShmemConf};
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

/// Segment layout: a 64-byte header, then positions (f32×3), normals (f32×3), indices (u32×3).
#[repr(C)]
#[derive(Clone, Copy, Default, Debug)]
struct Header {
    seq: u64,
    vertices: u64,
    triangles: u64,
    faces: u64,
    checksum: u64,
    _pad: [u64; 3],
}
const HDR: usize = std::mem::size_of::<Header>();

fn checksum(bytes: &[u8]) -> u64 {
    // Cheap, order-sensitive: sum of u64 words with a rotate.
    bytes.chunks_exact(8).fold(0u64, |h, c| h.rotate_left(5) ^ u64::from_le_bytes(c.try_into().unwrap()))
}

// ---------------------------------------------------------------- child

/// Flat mesh buffers as they go into the shared-memory segment.
struct MeshOut {
    positions: Vec<f32>,
    normals: Vec<f32>,
    indices: Vec<u32>,
    faces: usize,
}

/// A sphere meshed by the engine's OCCT kernel (0.5 rad angular deflection, as P1 used).
fn sphere_mesh(radius: f64, deflection: f64) -> Result<MeshOut, String> {
    use oc_kernel::Kernel;
    let k = oc_kernel_occt::OcctKernel::default();
    let body = k.primitive(&oc_kernel::Primitive::Sphere { center: [0.0; 3], radius }).map_err(|e| e.to_string())?.body;
    let m = k.tessellate(&body, &oc_kernel::MeshParams { linear: deflection, angular: 0.5 }).map_err(|e| e.to_string())?;
    Ok(MeshOut {
        positions: m.positions.iter().flat_map(|p| p.map(|x| x as f32)).collect(),
        normals: m.normals.iter().flatten().copied().collect(),
        indices: m.triangles.iter().flatten().copied().collect(),
        faces: m.faces.len(),
    })
}

fn child() {
    let stdin = std::io::stdin();
    let mut out = std::io::stdout().lock();
    let mut shm: Option<Shmem> = None;
    // Tessellation is cached per deflection: this spike measures transport, and OCCT's mesher
    // is far too slow at 1M triangles on one face to repeat it per request (see main()).
    let mut cache: Option<(f64, MeshOut)> = None;
    for line in stdin.lock().lines() {
        let line = line.unwrap();
        let mut it = line.split_whitespace();
        match it.next() {
            Some("mesh") => {
                let seq: u64 = it.next().unwrap().parse().unwrap();
                let deflection: f64 = it.next().unwrap().parse().unwrap();
                let t = Instant::now();
                if cache.as_ref().map_or(true, |(d, _)| *d != deflection) {
                    match sphere_mesh(50.0, deflection) {
                        Ok(m) => cache = Some((deflection, m)),
                        Err(e) => {
                            writeln!(out, "err {e}").unwrap();
                            out.flush().unwrap();
                            continue;
                        }
                    }
                }
                let m = &cache.as_ref().unwrap().1;
                let gen_ms = t.elapsed().as_secs_f64() * 1e3;
                let t = Instant::now();
                let body = (m.positions.len() + m.normals.len() + m.indices.len()) * 4;
                if shm.as_ref().map_or(true, |s| s.len() < HDR + body) {
                    // Grow by recreating; the parent remaps when the name changes.
                    shm = Some(ShmemConf::new().size((HDR + body).next_power_of_two()).create().unwrap());
                }
                let seg = shm.as_mut().unwrap();
                let base = seg.as_ptr();
                let bytes = unsafe { std::slice::from_raw_parts_mut(base.add(HDR), body) };
                let (p, rest) = bytes.split_at_mut(m.positions.len() * 4);
                let (n, i) = rest.split_at_mut(m.normals.len() * 4);
                p.copy_from_slice(bytemuck(&m.positions));
                n.copy_from_slice(bytemuck(&m.normals));
                i.copy_from_slice(bytemuck(&m.indices));
                let h = Header {
                    seq,
                    vertices: (m.positions.len() / 3) as u64,
                    triangles: (m.indices.len() / 3) as u64,
                    faces: m.faces as u64,
                    checksum: checksum(bytes),
                    _pad: [0; 3],
                };
                unsafe { std::ptr::write_volatile(base as *mut Header, h) };
                let write_ms = t.elapsed().as_secs_f64() * 1e3;
                writeln!(out, "ready {} {} {gen_ms:.2} {write_ms:.2}", seg.get_os_id(), seg.len()).unwrap();
                out.flush().unwrap();
            }
            Some("crash") => unsafe {
                // A wild pointer inside the kernel process, as a bad OCCT call would do.
                std::ptr::write_volatile(8 as *mut u64, 1);
            },
            Some("abort") => std::process::abort(),
            Some("hang") => loop {
                std::thread::sleep(Duration::from_secs(1));
            },
            Some("ping") => {
                writeln!(out, "pong").unwrap();
                out.flush().unwrap();
            }
            _ => {}
        }
    }
}

fn bytemuck<T: Copy>(v: &[T]) -> &[u8] {
    unsafe { std::slice::from_raw_parts(v.as_ptr() as *const u8, std::mem::size_of_val(v)) }
}

// ---------------------------------------------------------------- parent

struct Host {
    child: Child,
    stdin: ChildStdin,
    lines: mpsc::Receiver<String>,
    seg: Option<(String, Shmem)>,
    restarts: u32,
}

impl Host {
    fn spawn() -> (Child, ChildStdin, mpsc::Receiver<String>) {
        let mut child = Command::new(std::env::current_exe().unwrap())
            .arg("--child")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let stdin = child.stdin.take().unwrap();
        let stdout: ChildStdout = child.stdout.take().unwrap();
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            for l in BufReader::new(stdout).lines().map_while(Result::ok) {
                if tx.send(l).is_err() {
                    break;
                }
            }
        });
        (child, stdin, rx)
    }

    fn new() -> Self {
        let (child, stdin, lines) = Self::spawn();
        Host { child, stdin, lines, seg: None, restarts: 0 }
    }

    fn restart(&mut self) -> Duration {
        let t = Instant::now();
        let _ = self.child.kill();
        let _ = self.child.wait();
        let (c, i, l) = Self::spawn();
        (self.child, self.stdin, self.lines) = (c, i, l);
        self.seg = None;
        self.restarts += 1;
        // Ready once it answers.
        self.send("ping");
        let r = self.lines.recv_timeout(Duration::from_secs(10));
        assert_eq!(r.as_deref(), Ok("pong"));
        t.elapsed()
    }

    fn send(&mut self, cmd: &str) {
        // A dead child makes this fail; the reply wait reports it.
        let _ = writeln!(self.stdin, "{cmd}").and_then(|_| self.stdin.flush());
    }

    /// Sends a request and waits for one reply line; None if the child died or timed out.
    fn call(&mut self, cmd: &str, timeout: Duration) -> Option<String> {
        self.send(cmd);
        match self.lines.recv_timeout(timeout) {
            Ok(l) => Some(l),
            Err(_) => None,
        }
    }
}

struct Received {
    triangles: u64,
    gen_ms: f64,
    write_ms: f64,
    transfer_ms: f64,
}

fn request_mesh(h: &mut Host, seq: u64, deflection: f64) -> Option<Received> {
    let reply = h.call(&format!("mesh {seq} {deflection}"), Duration::from_secs(120))?;
    let t = Instant::now();
    let parts: Vec<&str> = reply.split_whitespace().collect();
    assert_eq!(parts[0], "ready", "{reply}");
    let (name, gen_ms, write_ms) = (parts[1].to_string(), parts[3].parse().unwrap(), parts[4].parse().unwrap());
    if h.seg.as_ref().map_or(true, |(n, _)| *n != name) {
        h.seg = Some((name.clone(), ShmemConf::new().os_id(&name).open().unwrap()));
    }
    let seg = &h.seg.as_ref().unwrap().1;
    let hdr = unsafe { std::ptr::read_volatile(seg.as_ptr() as *const Header) };
    assert_eq!(hdr.seq, seq);
    let body = ((hdr.vertices * 6 + hdr.triangles * 3) * 4) as usize;
    let src = unsafe { std::slice::from_raw_parts(seg.as_ptr().add(HDR), body) };
    // The parent's own copy: what a wgpu staging-buffer write would consume.
    let copy = src.to_vec();
    let transfer_ms = t.elapsed().as_secs_f64() * 1e3;
    assert_eq!(checksum(&copy), hdr.checksum, "torn or corrupt mesh");
    Some(Received { triangles: hdr.triangles, gen_ms, write_ms, transfer_ms })
}

fn main() {
    if std::env::args().any(|a| a == "--child") {
        return child();
    }
    let t = Instant::now();
    let mut h = Host::new();
    h.call("ping", Duration::from_secs(10)).expect("child did not start");
    println!("child start + OCCT ready: {:.1} ms", t.elapsed().as_secs_f64() * 1e3);

    // Find a deflection that gives ~1M triangles, then measure transfers.
    // Each step also records OCCT's tessellation time (BRepMesh, one spherical face).
    let mut deflection = 0.05;
    let mut r = request_mesh(&mut h, 1, deflection).unwrap();
    println!("  tessellate deflection {deflection:.4}: {} triangles in {:.0} ms", r.triangles, r.gen_ms);
    while r.triangles < 1_000_000 {
        deflection *= 0.7;
        r = request_mesh(&mut h, 1, deflection).unwrap();
        println!("  tessellate deflection {deflection:.4}: {} triangles in {:.0} ms", r.triangles, r.gen_ms);
    }
    println!("deflection {deflection:.4} -> {} triangles", r.triangles);
    let mut transfers = vec![];
    for seq in 2..12 {
        let r = request_mesh(&mut h, seq, deflection).unwrap();
        println!(
            "  seq {seq}: tessellate {:.1} ms, child write {:.1} ms, parent transfer+copy {:.2} ms",
            r.gen_ms, r.write_ms, r.transfer_ms
        );
        transfers.push(r.transfer_ms);
    }
    transfers.sort_by(f64::total_cmp);
    let mb = (r.triangles as f64 * 3.0 * 4.0 + r.triangles as f64 * 0.5 * 24.0) / 1e6;
    println!(
        "transfer of {} triangles (~{mb:.0} MB): median {:.2} ms, max {:.2} ms (target < 20 ms)",
        r.triangles, transfers[transfers.len() / 2], transfers[transfers.len() - 1]
    );

    // Crash recovery.
    for (cmd, timeout) in [("crash", 5), ("abort", 5), ("hang", 2)] {
        let t = Instant::now();
        let reply = h.call(cmd, Duration::from_secs(timeout));
        assert!(reply.is_none(), "{cmd}: unexpected reply {reply:?}");
        let detect = t.elapsed();
        let restart = h.restart();
        let ok = request_mesh(&mut h, 100, 0.5).is_some();
        println!(
            "{cmd}: detected in {:.0} ms, restart {:.1} ms, retry {}",
            detect.as_secs_f64() * 1e3,
            restart.as_secs_f64() * 1e3,
            if ok { "ok" } else { "FAILED" }
        );
        assert!(ok);
    }
    println!("restarts: {}; parent survived", h.restarts);
}
