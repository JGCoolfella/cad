//! The geometry host's side: read jobs from standard input, regenerate each part on this
//! process' kernel (`oc_asm::parallel::work`, the same code worker threads run), answer on
//! standard output. Returns at the end of the input (the app closed the pipe or exited).
use std::io::{self, BufReader, BufWriter, Write};
use std::sync::Arc;

use oc_asm::parallel::{work, PartJob};
use oc_graph::Regenerator;
use oc_kernel::Kernel;
use oc_kernel_occt::OcctKernel;

use crate::proto::{self, decode_job, encode_done, encode_failed, encode_hello, read_frame, write_frame};

/// Environment variable that lets tests make a host fail on purpose: with it set to `1`, a
/// part whose name is `fault:crash`, `fault:abort`, `fault:exit` or `fault:hang` makes the
/// host die that way (an access violation, an abort, an exit, or never answering). Never
/// set by the app.
pub const FAULTS: &str = "OC_HOST_FAULTS";

/// Runs the host loop; the process' exit code.
pub fn serve() -> i32 {
    oc_kernel_occt::quiet_messages();
    let faults = std::env::var(FAULTS).as_deref() == Ok("1");
    let mut input = BufReader::new(io::stdin().lock());
    let mut output = BufWriter::new(io::stdout().lock());
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
        let reply = match decode_job(&payload) {
            Err(e) => (proto::FAILED, encode_failed(0, &e)),
            Ok(job) => match oc_doc::load(&job.doc) {
                Err(e) => (proto::FAILED, encode_failed(job.id, &e.to_string())),
                Ok(loaded) => {
                    if faults {
                        fault(&loaded.doc.name);
                    }
                    let pj = PartJob { hash: String::new(), doc: Arc::new(loaded.doc), quality: job.quality };
                    // A fresh regenerator per part: nothing is kept between jobs.
                    let mut regen = Regenerator::with_rules(kernel.clone(), job.rules);
                    let r = work(&mut regen, &pj);
                    match r {
                        Ok(out) => (proto::DONE, encode_done(job.id, &out)),
                        Err(e) => (proto::FAILED, encode_failed(job.id, &e)),
                    }
                }
            },
        };
        if write_frame(&mut output, reply.0, &reply.1).and_then(|_| output.flush()).is_err() {
            return 1;
        }
    }
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
        _ => {}
    }
}

/// An access violation, as a bad OCCT call makes one (section 7: draft on a closed face).
#[allow(unsafe_code)]
fn crash() {
    // SAFETY: deliberately not safe: this kills the process, which is the point.
    unsafe { std::ptr::write_volatile(std::ptr::without_provenance_mut::<u64>(8), 1) };
}
