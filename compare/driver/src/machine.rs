//! The agreed machine (criteria.md): output slugs, power plan, the quiet check, CPU pinning
//! and the hardware record.
//!
//! The Win32 calls below are the driver's only `unsafe`; each states why it is sound.
#![allow(unsafe_code)]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

// --- output names ---

/// `--machine NAME`: `[a-z0-9-]+`.
pub fn validate_slug(slug: &str) -> Result<()> {
    if slug.is_empty()
        || !slug
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
    {
        bail!("--machine needs a slug of [a-z0-9-], got '{slug}'");
    }
    Ok(())
}

/// `<date>`, or `<date>-2`, `-3`, ... when a run of that day is already stored in `dir`.
pub fn output_stem(dir: &Path, date: &str) -> String {
    let taken = |stem: &str| dir.join(format!("{stem}.json")).exists();
    if !taken(date) {
        return date.to_string();
    }
    (2..)
        .map(|n| format!("{date}-{n}"))
        .find(|stem| !taken(stem))
        .unwrap_or_else(|| date.to_string())
}

/// `YYYY-MM-DD` (UTC) from the system clock.
pub fn today() -> String {
    let seconds = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    civil_date(i64::try_from(seconds / 86_400).unwrap_or(0))
}

/// Days since 1970-01-01 to `YYYY-MM-DD` (Howard Hinnant's civil-from-days).
fn civil_date(days: i64) -> String {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let day_of_era = z.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_part = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_part + 2) / 5 + 1;
    let month = if month_part < 10 {
        month_part + 3
    } else {
        month_part - 9
    };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02}")
}

// --- power plan ---

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Scheme {
    pub guid: String,
    pub name: String,
}

impl Scheme {
    /// `Name guid`, the form results record.
    pub fn label(&self) -> String {
        format!("{} {}", self.name, self.guid)
    }
}

/// `powercfg` behind a seam, so the guard is testable without touching the real plan.
pub trait PowerControl {
    fn active(&self) -> Result<Scheme>;
    /// `target` is a GUID or an alias such as `SCHEME_MIN`.
    fn set_active(&self, target: &str) -> Result<()>;
}

pub struct Powercfg;

/// The alias of the built-in High performance scheme.
pub const HIGH_PERFORMANCE: &str = "SCHEME_MIN";

fn is_guid(token: &str) -> bool {
    let groups: Vec<&str> = token.split('-').collect();
    groups.len() == 5
        && [8, 4, 4, 4, 12]
            .iter()
            .zip(&groups)
            .all(|(len, group)| group.len() == *len && group.bytes().all(|b| b.is_ascii_hexdigit()))
}

/// The GUID and the parenthesised name of `powercfg /getactivescheme` output; works on a
/// localized Windows because only the GUID shape and the parentheses are matched.
pub fn parse_active_scheme(text: &str) -> Option<Scheme> {
    let line = text
        .lines()
        .find(|line| line.split_whitespace().any(is_guid))?;
    let guid = line.split_whitespace().find(|t| is_guid(t))?.to_lowercase();
    let name = line
        .rfind('(')
        .and_then(|open| line[open + 1..].split(')').next())
        .unwrap_or("unnamed")
        .trim()
        .to_string();
    Some(Scheme { guid, name })
}

impl PowerControl for Powercfg {
    fn active(&self) -> Result<Scheme> {
        let output = Command::new("powercfg")
            .arg("/getactivescheme")
            .output()
            .context("cannot run powercfg")?;
        let text = String::from_utf8_lossy(&output.stdout);
        parse_active_scheme(&text).context("cannot read the active power scheme from powercfg")
    }

    fn set_active(&self, target: &str) -> Result<()> {
        let status = Command::new("powercfg")
            .args(["/setactive", target])
            .status()
            .context("cannot run powercfg")?;
        if !status.success() {
            bail!("powercfg /setactive {target} failed ({status})");
        }
        Ok(())
    }
}

/// Puts the saved scheme back and deletes the file. Used by the guard, by the Ctrl-C handler
/// and at every driver start (a killed driver leaves the file behind).
pub fn restore_from_file(control: &dyn PowerControl, file: &Path) -> Result<Option<String>> {
    let Ok(text) = fs::read_to_string(file) else {
        return Ok(None);
    };
    let guid = text.trim();
    if guid.is_empty() {
        let _ = fs::remove_file(file);
        return Ok(None);
    }
    control.set_active(guid)?;
    let _ = fs::remove_file(file);
    Ok(Some(guid.to_string()))
}

/// Restores the previous scheme when dropped: normal exit, error return and panic alike.
pub struct PowerGuard<'a> {
    control: &'a dyn PowerControl,
    file: PathBuf,
}

impl Drop for PowerGuard<'_> {
    fn drop(&mut self) {
        if let Err(error) = restore_from_file(self.control, &self.file) {
            eprintln!("warning: could not restore the power plan: {error:#}");
        }
    }
}

pub struct PowerSession<'a> {
    pub before: Scheme,
    pub during: Scheme,
    /// `None` with `--keep-power-plan`: nothing was switched, nothing is restored.
    pub guard: Option<PowerGuard<'a>>,
}

/// Restores a leftover scheme, then (unless `keep`) switches to High performance. A failed
/// switch refuses the run and leaves the plan as it was.
pub fn begin_power_session<'a>(
    control: &'a dyn PowerControl,
    file: &Path,
    keep: bool,
) -> Result<PowerSession<'a>> {
    if let Some(guid) = restore_from_file(control, file)? {
        eprintln!("restored the power scheme {guid} left behind by a killed run");
    }
    let before = control.active()?;
    if keep {
        return Ok(PowerSession {
            during: before.clone(),
            before,
            guard: None,
        });
    }
    if let Some(dir) = file.parent() {
        fs::create_dir_all(dir)?;
    }
    fs::write(file, &before.guid).context("cannot save the power scheme to restore")?;
    let switched = control
        .set_active(HIGH_PERFORMANCE)
        .and_then(|()| control.active());
    match switched {
        Ok(during) => Ok(PowerSession {
            before,
            during,
            guard: Some(PowerGuard {
                control,
                file: file.to_path_buf(),
            }),
        }),
        Err(error) => {
            // Anything half-done is undone; the plan was never confirmed switched.
            let _ = restore_from_file(control, file);
            Err(error.context("cannot switch to the High performance power scheme"))
        }
    }
}

static CTRL_C_RESTORE_FILE: OnceLock<PathBuf> = OnceLock::new();

/// On Ctrl-C and console close: restore the plan, then exit 130. The handler runs on its own
/// thread and cannot take arguments, hence the one process-wide path.
pub fn install_ctrl_c_restore(file: &Path) {
    let _ = CTRL_C_RESTORE_FILE.set(file.to_path_buf());
    set_console_handler();
}

fn on_interrupt() -> ! {
    if let Some(file) = CTRL_C_RESTORE_FILE.get() {
        let _ = restore_from_file(&Powercfg, file);
    }
    std::process::exit(130)
}

#[cfg(windows)]
unsafe extern "system" fn console_handler(_event: u32) -> i32 {
    on_interrupt()
}

fn set_console_handler() {
    #[cfg(windows)]
    {
        use windows_sys::Win32::System::Console::SetConsoleCtrlHandler;
        // SAFETY: `console_handler` is a valid `extern "system"` callback for the life of the
        // process and only touches a `OnceLock` and a child `powercfg`.
        unsafe {
            SetConsoleCtrlHandler(Some(console_handler), 1);
        }
    }
}

// --- the quiet check ---

/// One running process, as the quiet check sees it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProcessEntry {
    pub pid: u32,
    pub parent: u32,
    pub name: String,
}

/// `pid` and every parent above it. A cycle (pid reuse) ends the walk.
fn ancestor_chain(processes: &[ProcessEntry], pid: u32) -> Vec<u32> {
    let mut chain = vec![pid];
    let mut current = pid;
    while let Some(entry) = processes.iter().find(|p| p.pid == current) {
        if entry.parent == 0 || chain.contains(&entry.parent) {
            break;
        }
        chain.push(entry.parent);
        current = entry.parent;
    }
    chain
}

/// The build tools whose presence makes the timings meaningless. The driver's own ancestors
/// do not count: `cargo run` keeps its `cargo.exe` alive as the parent while the driver
/// runs, and that one is idle.
pub fn running_build_tools(processes: &[ProcessEntry], own_pid: u32) -> Vec<String> {
    let ancestors = ancestor_chain(processes, own_pid);
    let mut found: Vec<String> = processes
        .iter()
        .filter(|process| !ancestors.contains(&process.pid))
        .filter(|process| {
            let lower = process.name.to_lowercase();
            ["cargo", "rustc"]
                .iter()
                .any(|tool| lower == *tool || lower == format!("{tool}.exe"))
        })
        .map(|process| process.name.clone())
        .collect();
    found.sort();
    found.dedup();
    found
}

pub fn refuse_if_building(processes: &[ProcessEntry], own_pid: u32) -> Result<()> {
    let busy = running_build_tools(processes, own_pid);
    if busy.is_empty() {
        return Ok(());
    }
    bail!(
        "refusing to run while {} is running: wait for it to finish",
        busy.join(", ")
    )
}

/// Every running process.
pub fn running_processes() -> Vec<ProcessEntry> {
    #[cfg(windows)]
    {
        windows_processes()
    }
    #[cfg(not(windows))]
    {
        let Ok(entries) = fs::read_dir("/proc") else {
            return Vec::new();
        };
        entries
            .flatten()
            .filter_map(|entry| {
                let pid = entry.file_name().to_str()?.parse().ok()?;
                let name = fs::read_to_string(entry.path().join("comm")).ok()?;
                let stat = fs::read_to_string(entry.path().join("stat")).ok()?;
                // `pid (comm) state ppid ...`: the parent follows the last `)`.
                let after = stat.rsplit_once(')')?.1;
                let parent = after.split_whitespace().nth(1)?.parse().ok()?;
                Some(ProcessEntry {
                    pid,
                    parent,
                    name: name.trim().to_string(),
                })
            })
            .collect()
    }
}

#[cfg(windows)]
fn windows_processes() -> Vec<ProcessEntry> {
    use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW,
        TH32CS_SNAPPROCESS,
    };

    let mut processes = Vec::new();
    // SAFETY: the snapshot handle is checked and closed; `entry` is a zeroed plain-data struct
    // whose `dwSize` is set as the API requires, and outlives every call that fills it.
    unsafe {
        let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snapshot == INVALID_HANDLE_VALUE {
            return processes;
        }
        let mut entry: PROCESSENTRY32W = std::mem::zeroed();
        entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
        let mut more = Process32FirstW(snapshot, &mut entry) != 0;
        while more {
            let len = entry
                .szExeFile
                .iter()
                .position(|c| *c == 0)
                .unwrap_or(entry.szExeFile.len());
            processes.push(ProcessEntry {
                pid: entry.th32ProcessID,
                parent: entry.th32ParentProcessID,
                name: String::from_utf16_lossy(&entry.szExeFile[..len]),
            });
            more = Process32NextW(snapshot, &mut entry) != 0;
        }
        CloseHandle(snapshot);
    }
    processes
}

// --- pinning ---

/// Pins this process (children inherit) to logical CPU `cpu`.
pub fn pin_to_cpu(cpu: u32) -> Result<()> {
    if cpu >= usize::BITS {
        bail!("--cpu {cpu} is outside the affinity mask");
    }
    pin_mask(1usize << cpu)
}

fn pin_mask(mask: usize) -> Result<()> {
    #[cfg(windows)]
    {
        use windows_sys::Win32::System::Threading::{GetCurrentProcess, SetProcessAffinityMask};
        // SAFETY: the pseudo handle of the current process needs no closing; the mask is a value.
        let ok = unsafe { SetProcessAffinityMask(GetCurrentProcess(), mask) };
        if ok == 0 {
            bail!("SetProcessAffinityMask failed");
        }
        Ok(())
    }
    #[cfg(not(windows))]
    {
        let _ = mask;
        bail!("CPU pinning is only implemented on Windows; use --no-pin")
    }
}

// --- the hardware record ---

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct MachineRecord {
    pub name: String,
    pub cpu: String,
    pub logical_cpus: usize,
    pub ram_gib: u64,
    pub os: String,
    pub power_scheme_before: String,
    pub power_scheme_during: String,
    pub on_ac: Option<bool>,
    pub pinned_cpu: Option<u32>,
    pub priority: String,
    pub power_plan_kept: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct Toolchains {
    pub rustc: String,
    pub zig: String,
    pub node: String,
    pub msvc: String,
}

pub fn cpu_brand() -> String {
    #[cfg(target_arch = "x86_64")]
    {
        use core::arch::x86_64::__cpuid;
        let max = __cpuid(0x8000_0000).eax;
        if max < 0x8000_0004 {
            return "unknown".to_string();
        }
        let mut bytes = Vec::with_capacity(48);
        for leaf in 0x8000_0002..=0x8000_0004u32 {
            let r = __cpuid(leaf);
            for register in [r.eax, r.ebx, r.ecx, r.edx] {
                bytes.extend_from_slice(&register.to_le_bytes());
            }
        }
        let end = bytes.iter().position(|b| *b == 0).unwrap_or(bytes.len());
        String::from_utf8_lossy(&bytes[..end]).trim().to_string()
    }
    #[cfg(not(target_arch = "x86_64"))]
    {
        "unknown".to_string()
    }
}

pub fn ram_gib() -> u64 {
    #[cfg(windows)]
    {
        use windows_sys::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
        // SAFETY: `status` is a zeroed plain-data struct with `dwLength` set as the API requires.
        let total = unsafe {
            let mut status: MEMORYSTATUSEX = std::mem::zeroed();
            status.dwLength = std::mem::size_of::<MEMORYSTATUSEX>() as u32;
            (GlobalMemoryStatusEx(&mut status) != 0).then_some(status.ullTotalPhys)
        };
        total.map_or(0, |bytes| (bytes + (1 << 29)) >> 30)
    }
    #[cfg(not(windows))]
    {
        let meminfo = fs::read_to_string("/proc/meminfo").unwrap_or_default();
        let kib: u64 = meminfo
            .lines()
            .find_map(|l| l.strip_prefix("MemTotal:"))
            .and_then(|rest| rest.split_whitespace().next()?.parse().ok())
            .unwrap_or(0);
        ((kib << 10) + (1 << 29)) >> 30
    }
}

pub fn on_ac_power() -> Option<bool> {
    #[cfg(windows)]
    {
        use windows_sys::Win32::System::Power::{GetSystemPowerStatus, SYSTEM_POWER_STATUS};
        // SAFETY: `status` is a zeroed plain-data struct the call fills.
        let status = unsafe {
            let mut status: SYSTEM_POWER_STATUS = std::mem::zeroed();
            (GetSystemPowerStatus(&mut status) != 0).then_some(status)
        }?;
        match status.ACLineStatus {
            0 => Some(false),
            1 => Some(true),
            _ => None,
        }
    }
    #[cfg(not(windows))]
    {
        None
    }
}

fn run_text(program: &str, args: &[&str]) -> Option<String> {
    let output = Command::new(program).args(args).output().ok()?;
    let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
    (output.status.success() && !text.is_empty()).then_some(text)
}

pub fn os_name() -> String {
    if cfg!(windows) {
        // `ver` prints "Microsoft Windows [Version 10.0.26300.1234]"; the edition is not in it.
        return run_text("cmd", &["/c", "ver"]).unwrap_or_else(|| "Windows".to_string());
    }
    let release = fs::read_to_string("/etc/os-release").unwrap_or_default();
    let pretty = release
        .lines()
        .find_map(|l| l.strip_prefix("PRETTY_NAME="))
        .unwrap_or("Linux")
        .trim_matches('"')
        .to_string();
    let kernel = run_text("uname", &["-r"]).unwrap_or_default();
    format!("{pretty} {kernel}").trim().to_string()
}

pub fn toolchains(zig: &Path) -> Toolchains {
    let version_of = |text: Option<String>, word: usize| {
        text.and_then(|t| t.split_whitespace().nth(word).map(str::to_string))
            .unwrap_or_else(|| "unknown".to_string())
    };
    let vswhere = r"C:\Program Files (x86)\Microsoft Visual Studio\Installer\vswhere.exe";
    Toolchains {
        rustc: version_of(run_text("rustc", &["-V"]), 1),
        zig: run_text(&zig.to_string_lossy(), &["version"])
            .unwrap_or_else(|| "unknown".to_string()),
        node: run_text("node", &["-v"]).unwrap_or_else(|| "unknown".to_string()),
        msvc: run_text(
            vswhere,
            &[
                "-products",
                "*",
                "-latest",
                "-property",
                "catalog_productDisplayVersion",
            ],
        )
        .unwrap_or_else(|| "unknown".to_string()),
    }
}

pub fn logical_cpus() -> usize {
    std::thread::available_parallelism().map_or(1, usize::from)
}

#[cfg(test)]
#[path = "machine_tests.rs"]
mod machine_tests;
