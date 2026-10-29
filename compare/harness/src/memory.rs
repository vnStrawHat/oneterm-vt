//! Process memory for the `verify` pass: private bytes and their peak.

/// A reading of this process's memory, in bytes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Memory {
    /// Private bytes now (Windows `PrivateUsage`, Linux `VmRSS`).
    pub private: u64,
    /// Highest private bytes so far (Windows `PeakPagefileUsage`, Linux `VmHWM`).
    pub peak: u64,
}

/// Reads this process's memory; zeros when the platform call fails (reported, never fatal).
pub fn sample() -> Memory {
    read_counters().unwrap_or_default()
}

// The one FFI call of the harness: `K32GetProcessMemoryInfo` fills a caller-owned struct.
#[cfg(windows)]
#[allow(unsafe_code)]
fn read_counters() -> Option<Memory> {
    use windows_sys::Win32::System::ProcessStatus::{
        K32GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS_EX,
    };
    use windows_sys::Win32::System::Threading::GetCurrentProcess;

    // SAFETY: `counters` is a zeroed plain-data struct of exactly the size passed in `cb`, and
    // outlives the call; `GetCurrentProcess` returns a pseudo handle that needs no closing.
    let counters = unsafe {
        let mut counters: PROCESS_MEMORY_COUNTERS_EX = std::mem::zeroed();
        let size = std::mem::size_of::<PROCESS_MEMORY_COUNTERS_EX>() as u32;
        counters.cb = size;
        let ok = K32GetProcessMemoryInfo(GetCurrentProcess(), (&raw mut counters).cast(), size);
        (ok != 0).then_some(counters)
    }?;
    Some(Memory {
        private: counters.PrivateUsage as u64,
        peak: counters.PeakPagefileUsage as u64,
    })
}

#[cfg(not(windows))]
fn read_counters() -> Option<Memory> {
    parse_proc_status(&std::fs::read_to_string("/proc/self/status").ok()?)
}

/// `VmRSS` and `VmHWM` (kB) of `/proc/self/status` text.
#[cfg(any(not(windows), test))]
fn parse_proc_status(status: &str) -> Option<Memory> {
    let kib = |key: &str| -> Option<u64> {
        let line = status.lines().find(|line| line.starts_with(key))?;
        line.split_whitespace().nth(1)?.parse::<u64>().ok()
    };
    Some(Memory {
        private: kib("VmRSS:")? * 1024,
        peak: kib("VmHWM:")? * 1024,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn proc_status_lines_become_bytes() {
        let status = "Name:\tx\nVmHWM:\t    2048 kB\nVmRSS:\t    1024 kB\n";
        assert_eq!(
            parse_proc_status(status),
            Some(Memory {
                private: 1024 * 1024,
                peak: 2048 * 1024
            })
        );
        assert_eq!(parse_proc_status("Name:\tx\n"), None);
    }

    #[test]
    fn this_process_has_private_bytes() {
        let now = sample();
        assert!(now.private > 0);
        assert!(now.peak >= now.private / 2);
    }
}
