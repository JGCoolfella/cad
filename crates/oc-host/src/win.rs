//! Windows specifics of the geometry hosts (G3d).
//!
//! - **The protocol's own pipe.** A host's replies travel on an anonymous pipe made for it, not
//!   on its standard output (which goes to `NUL`), so nothing the kernel or a library prints
//!   can break the protocol. The app makes the pipe after starting the host, duplicates the
//!   write end into the host ([`give`]) and names it in the first frame on the host's input
//!   (`proto::OUTPUT`). Nothing is inherited, so hosts started at the same time from several
//!   threads never hold each other's pipes (a pipe held open elsewhere would hide a host's
//!   death from the app).
//! - **A job object that kills on close.** Every host is put in the pool's [`Job`]; when its
//!   last handle closes (the app ends, also when it is killed or crashes) Windows ends every
//!   process in it, so no host outlives the app, not even one hanging in a part.
#![allow(unsafe_code)]

use std::fs::File;
use std::io;
use std::os::windows::io::{AsRawHandle, FromRawHandle, RawHandle};
use std::ptr::{null, null_mut};

use windows_sys::Win32::Foundation::{CloseHandle, DuplicateHandle, DUPLICATE_SAME_ACCESS, HANDLE};
use windows_sys::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation, SetInformationJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
};
use windows_sys::Win32::System::Pipes::CreatePipe;
use windows_sys::Win32::System::Threading::GetCurrentProcess;

/// An anonymous pipe, neither end inheritable: (read end, write end).
pub fn pipe() -> io::Result<(File, File)> {
    let (mut r, mut w): (HANDLE, HANDLE) = (null_mut(), null_mut());
    // SAFETY: plain out-pointers; on success both handles are new and owned by the files.
    if unsafe { CreatePipe(&mut r, &mut w, null(), 0) } == 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: as above.
    Ok(unsafe { (File::from_raw_handle(r as RawHandle), File::from_raw_handle(w as RawHandle)) })
}

/// Duplicates `h` into `process` (a child's process handle); the handle's value there.
pub fn give(h: &File, process: RawHandle) -> io::Result<u64> {
    let mut out: HANDLE = null_mut();
    // SAFETY: valid handles; the duplicate belongs to the other process.
    let ok = unsafe { DuplicateHandle(GetCurrentProcess(), h.as_raw_handle() as HANDLE, process as HANDLE, &mut out, 0, 0, DUPLICATE_SAME_ACCESS) };
    if ok == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(out as usize as u64)
}

/// In the host: the handle the app gave it ([`give`]), as a file.
///
/// # Safety
/// `value` must be a handle of this process that nothing else owns.
pub unsafe fn take(value: u64) -> File {
    // SAFETY: the caller's.
    unsafe { File::from_raw_handle(value as usize as RawHandle) }
}

/// A job object whose processes end when its last handle closes.
pub struct Job(HANDLE);

// SAFETY: a job handle may be used and closed from any thread.
unsafe impl Send for Job {}
unsafe impl Sync for Job {}

impl Job {
    /// `None` when Windows refuses (the hosts then still end when their input closes, unless
    /// one hangs).
    pub fn new() -> Option<Job> {
        // SAFETY: an unnamed job with default security.
        let h = unsafe { CreateJobObjectW(null(), null()) };
        if h.is_null() {
            return None;
        }
        let job = Job(h);
        // SAFETY: zeroed is a valid value of this plain C struct.
        let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { std::mem::zeroed() };
        info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        let size = std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32;
        // SAFETY: the struct and its size match the information class.
        let ok = unsafe { SetInformationJobObject(job.0, JobObjectExtendedLimitInformation, (&info as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(), size) };
        (ok != 0).then_some(job)
    }

    /// Puts a process (a child's process handle) in the job.
    pub fn add(&self, process: RawHandle) -> bool {
        // SAFETY: valid handles.
        unsafe { AssignProcessToJobObject(self.0, process as HANDLE) != 0 }
    }
}

impl Drop for Job {
    fn drop(&mut self) {
        // SAFETY: owned handle, closed once. Closing the last handle ends the processes.
        unsafe { CloseHandle(self.0) };
    }
}
