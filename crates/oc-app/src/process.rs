//! The process' memory and thread count, for native scripts (G3a: `scale.js` records what
//! opening a 10 000-component assembly costs).
use serde::Serialize;

#[derive(Serialize, Clone, Debug, Default, PartialEq)]
pub struct ProcessStats {
    /// Working set and its peak, MB.
    #[serde(rename = "workingSetMb")]
    pub working_set_mb: f64,
    #[serde(rename = "peakMb")]
    pub peak_mb: f64,
    /// Threads of this process.
    pub threads: u32,
    /// Logical processors the process may use.
    pub cpus: usize,
}

#[cfg(windows)]
#[allow(unsafe_code)]
pub fn stats() -> ProcessStats {
    use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{CreateToolhelp32Snapshot, Thread32First, Thread32Next, TH32CS_SNAPTHREAD, THREADENTRY32};
    use windows_sys::Win32::System::ProcessStatus::{K32GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS};
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, GetCurrentProcessId};
    let mut out = ProcessStats { cpus: std::thread::available_parallelism().map_or(1, |n| n.get()), ..Default::default() };
    // SAFETY: Win32 calls filling structs of the sizes given; the snapshot handle is closed.
    unsafe {
        let mut c: PROCESS_MEMORY_COUNTERS = std::mem::zeroed();
        c.cb = std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32;
        if K32GetProcessMemoryInfo(GetCurrentProcess(), &mut c, c.cb) != 0 {
            out.working_set_mb = c.WorkingSetSize as f64 / 1048576.0;
            out.peak_mb = c.PeakWorkingSetSize as f64 / 1048576.0;
        }
        let snap = CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0);
        if snap != INVALID_HANDLE_VALUE {
            let pid = GetCurrentProcessId();
            let mut e: THREADENTRY32 = std::mem::zeroed();
            e.dwSize = std::mem::size_of::<THREADENTRY32>() as u32;
            let mut ok = Thread32First(snap, &mut e) != 0;
            while ok {
                if e.th32OwnerProcessID == pid {
                    out.threads += 1;
                }
                ok = Thread32Next(snap, &mut e) != 0;
            }
            CloseHandle(snap);
        }
    }
    out
}

#[cfg(not(windows))]
pub fn stats() -> ProcessStats {
    ProcessStats { cpus: std::thread::available_parallelism().map_or(1, |n| n.get()), ..Default::default() }
}
