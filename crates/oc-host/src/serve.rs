//! The geometry host's side: read jobs from standard input, regenerate each part on this
//! process' kernel (`oc_asm::parallel::work`, the same code worker threads run), answer on
//! the reply pipe the app gave (Windows; standard output elsewhere). Returns at the end of
//! the input (the app closed the pipe or exited).
use std::io::{self, BufReader, BufWriter, Read, Write};
use std::sync::Arc;

use oc_asm::parallel::{work, PartJob};
use oc_graph::Regenerator;
use oc_kernel::Kernel;
use oc_kernel_occt::OcctKernel;

use crate::proto::{self, decode_job, encode_done, encode_failed, encode_hello, read_frame, write_frame};

/// Environment variable that lets tests make a host fail on purpose: with it set to `1`, a
/// part whose name is `fault:crash`, `fault:abort`, `fault:exit` or `fault:hang` makes the
/// host die that way (an access violation, an abort, an exit, or never answering), and
/// `fault:print` makes it print to its standard output (Rust's and the C runtime's) before
/// building the part as usual. Never set by the app.
pub const FAULTS: &str = "OC_HOST_FAULTS";

/// Runs the host loop; the process' exit code.
pub fn serve() -> i32 {
    oc_kernel_occt::quiet_messages();
    let faults = std::env::var(FAULTS).as_deref() == Ok("1");
    let mut input = BufReader::new(io::stdin().lock());
    let Some(out) = reply_stream(&mut input) else { return 2 };
    let mut output = BufWriter::new(out);
    let kernel = OcctKernel::default();
    if write_frame(&mut output, proto::HELLO, &encode_hello(&kernel.name())).is_err() {
        return 1;
    }
    loop {
        let (kind, payload) = match read_frame(&mut input) {
            Ok(Some(f)) => f,
            Ok(None) => return 0,
            Err(_) => return 2,
        };
        if kind != proto::JOB {
            return 2;
        }
        let reply = answer(&kernel, &payload, faults);
        if write_frame(&mut output, reply.0, &reply.1).and_then(|_| output.flush()).is_err() {
            return 1;
        }
    }
}

/// One job's reply frame (kind and payload): the part regenerated on `kernel` with a fresh
/// regenerator, nothing kept between jobs. Also the browser's host workers' job (W1b).
pub fn answer(kernel: &OcctKernel, payload: &[u8], faults: bool) -> (u32, Vec<u8>) {
    match decode_job(payload) {
        Err(e) => (proto::FAILED, encode_failed(0, &e)),
        Ok(job) => match oc_doc::load(&job.doc) {
            Err(e) => (proto::FAILED, encode_failed(job.id, &e.to_string())),
            Ok(loaded) => {
                if faults {
                    fault(&loaded.doc.name);
                }
                let pj = PartJob { hash: String::new(), doc: Arc::new(loaded.doc), quality: job.quality };
                let mut regen = Regenerator::with_rules(kernel.clone(), job.rules);
                match work(&mut regen, &pj) {
                    Ok(out) => (proto::DONE, encode_done(job.id, &out)),
                    Err(e) => (proto::FAILED, encode_failed(job.id, &e)),
                }
            }
        },
    }
}

/// Where replies go: the pipe named by the app's first frame (Windows, G3d).
#[cfg(windows)]
#[allow(unsafe_code)]
fn reply_stream(input: &mut impl Read) -> Option<Box<dyn Write>> {
    match read_frame(input) {
        Ok(Some((proto::OUTPUT, p))) if p.len() == 8 => {
            let handle = u64::from_le_bytes(p.try_into().ok()?);
            // SAFETY: the app duplicated the pipe's write end into this process for us.
            Some(Box::new(unsafe { crate::win::take(handle) }))
        }
        _ => None,
    }
}

#[cfg(not(windows))]
fn reply_stream(_: &mut impl Read) -> Option<Box<dyn Write>> {
    Some(Box::new(io::stdout()))
}

/// Test faults (see [`FAULTS`]).
fn fault(name: &str) {
    match name {
        "fault:crash" => crash(),
        "fault:abort" => std::process::abort(),
        "fault:exit" => std::process::exit(3),
        "fault:hang" => loop {
            std::thread::sleep(std::time::Duration::from_secs(1));
        },
        "fault:print" => print_to_stdout(),
        _ => {}
    }
}

/// What a chatty kernel might do: text and frame-like bytes on standard output, through
/// Rust's handle and the C runtime's (OCCT's printers write through the latter).
#[allow(unsafe_code)]
fn print_to_stdout() {
    use std::io::Write;
    let mut out = io::stdout();
    let _ = out.write_all(b"OCHP\x03\x00\x00\x00 a kernel message\n");
    let _ = out.flush();
    println!("Warning: a message from the geometry kernel");
    extern "C" {
        fn puts(s: *const std::ffi::c_char) -> i32;
        fn fflush(f: *mut std::ffi::c_void) -> i32;
    }
    // SAFETY: a NUL-terminated string; `fflush(NULL)` flushes every C stream.
    unsafe {
        puts(c"OCHP from the C runtime".as_ptr());
        fflush(std::ptr::null_mut());
    }
}

/// An access violation, as a bad OCCT call makes one (section 7: draft on a closed face).
#[cfg(not(target_family = "wasm"))]
#[allow(unsafe_code)]
fn crash() {
    // SAFETY: deliberately not safe: this kills the process, which is the point.
    unsafe { std::ptr::write_volatile(std::ptr::without_provenance_mut::<u64>(8), 1) };
}

/// WebAssembly has no access violations (address 8 is ordinary memory): a bad call there is a
/// trap, which ends the host worker as a crash ends a host process.
#[cfg(target_family = "wasm")]
fn crash() {
    core::arch::wasm32::unreachable()
}
