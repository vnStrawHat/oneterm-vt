//! The console host: choosing it, creating the pseudo-console, starting the child behind it.

#![expect(
    unsafe_code,
    reason = "Win32 FFI: ConPTY, pipes, CreateProcessW, dynamic conpty.dll"
)]

use std::collections::{BTreeMap, HashMap, btree_map::Entry};
use std::ffi::{CStr, OsStr, OsString, c_void};
use std::io;
use std::mem;
use std::os::windows::ffi::OsStrExt;
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::path::Path;
use std::ptr;
use std::sync::Once;

use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, HMODULE, S_OK};
use windows_sys::Win32::Security::SECURITY_ATTRIBUTES;
use windows_sys::Win32::System::Console::{
    COORD, ClosePseudoConsole, CreatePseudoConsole, HPCON, ResizePseudoConsole,
};
use windows_sys::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};
use windows_sys::Win32::System::Pipes::CreatePipe;
use windows_sys::Win32::System::Threading::{
    CREATE_UNICODE_ENVIRONMENT, CreateProcessW, DeleteProcThreadAttributeList,
    EXTENDED_STARTUPINFO_PRESENT, InitializeProcThreadAttributeList,
    PROC_THREAD_ATTRIBUTE_PSEUDOCONSOLE, PROCESS_INFORMATION, STARTF_USESTDHANDLES, STARTUPINFOEXW,
    UpdateProcThreadAttribute,
};

use super::win32_string;
use crate::pty::{DROPPED_PARENT_ENV, GlyphWidth};

type CreateFn = unsafe extern "system" fn(COORD, HANDLE, HANDLE, u32, *mut HPCON) -> i32;
type ResizeFn = unsafe extern "system" fn(HPCON, COORD) -> i32;
type CloseFn = unsafe extern "system" fn(HPCON);
/// What `GetProcAddress` returns before it is given a signature.
type RawExport = unsafe extern "system" fn() -> isize;

/// `PSEUDOCONSOLE_GLYPH_WIDTH_WCSWIDTH`.
const GLYPH_WIDTH_WCSWIDTH: u32 = 0x10;
/// `PSEUDOCONSOLE_GLYPH_WIDTH_GRAPHEMES`.
const GLYPH_WIDTH_GRAPHEMES: u32 = 0x08;

fn glyph_flag(width: GlyphWidth) -> u32 {
    match width {
        GlyphWidth::WcsWidth => GLYPH_WIDTH_WCSWIDTH,
        GlyphWidth::Graphemes => GLYPH_WIDTH_GRAPHEMES,
    }
}

/// Where the pseudo-console functions came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ConptyBackend {
    /// `conpty.dll` beside the executable (with its matching `OpenConsole.exe`).
    Bundled,
    /// The kernel32 functions of the running Windows.
    System,
}

impl ConptyBackend {
    fn name(self) -> &'static str {
        match self {
            Self::Bundled => "bundled",
            Self::System => "system",
        }
    }
}

/// The three pseudo-console functions of one host.
#[derive(Clone, Copy)]
struct ConptyApi {
    create: CreateFn,
    resize: ResizeFn,
    close: CloseFn,
    backend: ConptyBackend,
}

impl ConptyApi {
    /// The bundled host when `conpty.dll` sits beside the executable, else the inbox one. The
    /// bundled host passes sequences (Sixel) that the inbox `conhost.exe` swallows.
    fn resolve() -> Self {
        match std::env::current_exe() {
            Ok(exe) => exe.parent().map_or_else(Self::system, Self::resolve_in),
            Err(cause) => {
                log::warn!("conpty: cannot locate the executable ({cause}); using the system host");
                Self::system()
            }
        }
    }

    fn resolve_in(directory: &Path) -> Self {
        let api = Self::bundled(&directory.join("conpty.dll")).unwrap_or_else(Self::system);
        static LOG_BACKEND: Once = Once::new();
        LOG_BACKEND.call_once(|| log::info!("conpty: {}", api.backend.name()));
        api
    }

    fn system() -> Self {
        Self {
            create: CreatePseudoConsole,
            resize: ResizePseudoConsole,
            close: ClosePseudoConsole,
            backend: ConptyBackend::System,
        }
    }

    /// `None` when the file is missing or lacks an export (the latter is logged).
    fn bundled(path: &Path) -> Option<Self> {
        if !path.is_file() {
            return None;
        }
        let wide = win32_string(path.as_os_str());
        // SAFETY: `wide` is NUL-terminated and alive for the call. The module is never
        // freed, so the function pointers resolved below stay valid for the process.
        let module = unsafe { LoadLibraryW(wide.as_ptr()) };
        if module.is_null() {
            log::warn!(
                "conpty: cannot load {}: {}; using the system host",
                path.display(),
                io::Error::last_os_error()
            );
            return None;
        }
        let create = export(module, c"CreatePseudoConsole", path)?;
        let resize = export(module, c"ResizePseudoConsole", path)?;
        let close = export(module, c"ClosePseudoConsole", path)?;
        // SAFETY: the module exports the kernel32 ConPTY functions under the same names, with
        // the same `extern "system"` signatures.
        let api = unsafe {
            Self {
                create: mem::transmute::<RawExport, CreateFn>(create),
                resize: mem::transmute::<RawExport, ResizeFn>(resize),
                close: mem::transmute::<RawExport, CloseFn>(close),
                backend: ConptyBackend::Bundled,
            }
        };
        Some(api)
    }
}

fn export(module: HMODULE, name: &CStr, path: &Path) -> Option<RawExport> {
    // SAFETY: `module` is non-null and never freed; `name` is NUL-terminated.
    let found = unsafe { GetProcAddress(module, name.as_ptr().cast()) };
    if found.is_none() {
        log::warn!(
            "conpty: {} has no export {}; using the system host",
            path.display(),
            name.to_string_lossy()
        );
    }
    found
}

fn os_error(call: &str) -> io::Error {
    let cause = io::Error::last_os_error();
    io::Error::new(cause.kind(), format!("{call} failed: {cause}"))
}

fn hresult_error(call: &str, code: i32) -> String {
    format!("{call} failed (HRESULT 0x{:08X})", code as u32)
}

/// An open pseudo-console. Closing it ends the session for the client behind it.
pub(super) struct Conpty {
    handle: HPCON,
    api: ConptyApi,
}

impl Conpty {
    pub(super) fn resize(&self, size: COORD) -> io::Result<()> {
        // SAFETY: `handle` stays live until `Conpty::drop`.
        let code = unsafe { (self.api.resize)(self.handle, size) };
        if code == S_OK {
            return Ok(());
        }
        Err(io::Error::other(hresult_error(
            &format!("ResizePseudoConsole to {}x{}", size.X, size.Y),
            code,
        )))
    }
}

impl Drop for Conpty {
    fn drop(&mut self) {
        // SAFETY: `handle` is live and is closed exactly once, here. Before build 26100 the
        // call waits until the output pipe is drained or closed; `PseudoConsole` drops its
        // reader first for that reason.
        unsafe { (self.api.close)(self.handle) };
    }
}

/// Everything `spawn` needs, already converted to Win32 strings.
pub(super) struct SpawnRequest<'a> {
    /// Only for the error message.
    pub program: &'a str,
    /// Mutable and NUL-terminated: `CreateProcessW` may write to it.
    pub command_line: Vec<u16>,
    /// NUL-terminated.
    pub working_directory: Option<Vec<u16>>,
    /// From [`environment_block`].
    pub environment: Vec<u16>,
    pub size: COORD,
    pub glyph_width: GlyphWidth,
}

/// A started child behind its console, with our ends of the two pipes.
pub(super) struct Spawned {
    pub conpty: Conpty,
    pub process: OwnedHandle,
    pub conout: OwnedHandle,
    pub conin: OwnedHandle,
}

pub(super) fn spawn(request: SpawnRequest<'_>) -> io::Result<Spawned> {
    let api = ConptyApi::resolve();
    let (conout, conout_host) = anonymous_pipe()?;
    let (conin_host, conin) = anonymous_pipe()?;

    let mut handle: HPCON = 0;
    // SAFETY: both host handles are open for the call; `handle` is a live out-parameter.
    let code = unsafe {
        (api.create)(
            request.size,
            conin_host.as_raw_handle(),
            conout_host.as_raw_handle(),
            glyph_flag(request.glyph_width),
            &mut handle,
        )
    };
    if code != S_OK {
        return Err(io::Error::other(hresult_error("CreatePseudoConsole", code)));
    }
    // The host duplicated its ends; keeping ours would stop the pipes from ever reporting EOF.
    drop(conin_host);
    drop(conout_host);
    let conpty = Conpty { handle, api };

    match start_process(&conpty, request) {
        Ok(process) => Ok(Spawned {
            conpty,
            process,
            conout,
            conin,
        }),
        Err(cause) => {
            // Break the pipes before the console closes: a host before build 26100 waits for
            // its output to be drained or closed.
            drop(conout);
            drop(conin);
            drop(conpty);
            Err(cause)
        }
    }
}

fn start_process(conpty: &Conpty, mut request: SpawnRequest<'_>) -> io::Result<OwnedHandle> {
    // SAFETY: a plain C struct; all-zero is a valid value.
    let mut startup: STARTUPINFOEXW = unsafe { mem::zeroed() };
    // SAFETY: a plain C struct; all-zero is a valid value.
    let mut info: PROCESS_INFORMATION = unsafe { mem::zeroed() };
    startup.StartupInfo.cb = size_of::<STARTUPINFOEXW>() as u32;
    // Null standard handles: the child inherits none of ours.
    startup.StartupInfo.dwFlags |= STARTF_USESTDHANDLES;
    let mut attributes = ProcThreadAttributeList::with_console(conpty.handle)?;
    startup.lpAttributeList = attributes.as_mut_ptr();
    let directory = request
        .working_directory
        .as_ref()
        .map_or(ptr::null(), |directory| directory.as_ptr());

    // SAFETY: the command line is writable and NUL-terminated; the environment is a
    // double-NUL-terminated UTF-16 block and `CREATE_UNICODE_ENVIRONMENT` says so; the
    // directory is null or NUL-terminated; `startup` is a `STARTUPINFOEXW` (flagged by
    // `EXTENDED_STARTUPINFO_PRESENT`) whose attribute list holds the live console; `info` is
    // a live out-parameter. Everything outlives the call.
    let created = unsafe {
        CreateProcessW(
            ptr::null(),
            request.command_line.as_mut_ptr(),
            ptr::null(),
            ptr::null(),
            0,
            EXTENDED_STARTUPINFO_PRESENT | CREATE_UNICODE_ENVIRONMENT,
            request.environment.as_ptr().cast::<c_void>(),
            directory,
            &startup.StartupInfo,
            &mut info,
        )
    };
    drop(attributes);
    if created == 0 {
        let cause = io::Error::last_os_error();
        return Err(io::Error::new(
            cause.kind(),
            format!("cannot start '{}': {cause}", request.program),
        ));
    }
    // SAFETY: `CreateProcessW` succeeded, so both handles are open and owned by us alone;
    // the thread handle is closed here, the process handle is owned from here on.
    unsafe {
        CloseHandle(info.hThread);
        Ok(OwnedHandle::from_raw_handle(info.hProcess))
    }
}

fn anonymous_pipe() -> io::Result<(OwnedHandle, OwnedHandle)> {
    let attributes = SECURITY_ATTRIBUTES {
        nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: ptr::null_mut(),
        bInheritHandle: 0,
    };
    let mut read: HANDLE = ptr::null_mut();
    let mut write: HANDLE = ptr::null_mut();
    // SAFETY: both out-pointers are live for the call; `attributes` outlives it.
    if unsafe { CreatePipe(&mut read, &mut write, &attributes, 0) } == 0 {
        return Err(os_error("CreatePipe"));
    }
    // SAFETY: `CreatePipe` succeeded: both handles are open and owned by nobody else.
    Ok(unsafe {
        (
            OwnedHandle::from_raw_handle(read),
            OwnedHandle::from_raw_handle(write),
        )
    })
}

/// A `PROC_THREAD_ATTRIBUTE_LIST` carrying the pseudo-console, deleted on drop.
struct ProcThreadAttributeList {
    /// `u64` elements keep the buffer 8-byte aligned, as the list structure needs.
    buffer: Vec<u64>,
}

impl ProcThreadAttributeList {
    fn with_console(console: HPCON) -> io::Result<Self> {
        let mut size = 0usize;
        // SAFETY: a null list with a live size out-parameter only asks for the required size.
        let probe = unsafe { InitializeProcThreadAttributeList(ptr::null_mut(), 1, 0, &mut size) };
        if probe != 0 || size == 0 {
            return Err(io::Error::other(
                "InitializeProcThreadAttributeList did not report a buffer size",
            ));
        }
        let mut buffer = vec![0u64; size.div_ceil(size_of::<u64>())];
        // SAFETY: the buffer is at least `size` bytes and 8-byte aligned.
        let initialised = unsafe {
            InitializeProcThreadAttributeList(buffer.as_mut_ptr().cast(), 1, 0, &mut size)
        };
        if initialised == 0 {
            return Err(os_error("InitializeProcThreadAttributeList"));
        }
        // Only an initialised list may be deleted, so the guard exists from here on.
        let mut list = Self { buffer };
        // SAFETY: the list was just initialised with room for one attribute. The value is the
        // console handle itself, `size_of::<HPCON>()` bytes, and the caller keeps the console
        // alive until `CreateProcessW` returned.
        let updated = unsafe {
            UpdateProcThreadAttribute(
                list.as_mut_ptr(),
                0,
                PROC_THREAD_ATTRIBUTE_PSEUDOCONSOLE as usize,
                console as *const c_void,
                size_of::<HPCON>(),
                ptr::null_mut(),
                ptr::null(),
            )
        };
        if updated == 0 {
            return Err(os_error("UpdateProcThreadAttribute"));
        }
        Ok(list)
    }

    fn as_mut_ptr(&mut self) -> *mut c_void {
        self.buffer.as_mut_ptr().cast()
    }
}

impl Drop for ProcThreadAttributeList {
    fn drop(&mut self) {
        // SAFETY: only an initialised list is ever wrapped
        // and it is deleted once, before its buffer frees.
        unsafe { DeleteProcThreadAttributeList(self.as_mut_ptr()) };
    }
}

/// ASCII-uppercased UTF-16 of a variable name: the key the OS compares and sorts by.
fn fold_name(name: &OsStr) -> Vec<u16> {
    name.encode_wide()
        .map(|unit| {
            if (u16::from(b'a')..=u16::from(b'z')).contains(&unit) {
                unit - 0x20
            } else {
                unit
            }
        })
        .collect()
}

/// The child's environment block: the parent's variables minus the launching terminal's
/// identity, then `custom` on top, sorted case-insensitively as `CreateProcessW` requires.
/// Always a block, never null: null would copy the parent with the dropped names.
// ponytail: ASCII case folding; non-ASCII names sort by code unit.
pub(super) fn environment_block(
    custom: &HashMap<String, String>,
    parent: impl IntoIterator<Item = (OsString, OsString)>,
) -> Vec<u16> {
    let mut merged: BTreeMap<Vec<u16>, (OsString, OsString)> = BTreeMap::new();
    // `HashMap` order is arbitrary: sort so that a case-insensitive clash is decided the same
    // way every time.
    let mut custom: Vec<_> = custom.iter().collect();
    custom.sort();
    for (key, value) in custom {
        match merged.entry(fold_name(OsStr::new(key))) {
            Entry::Vacant(slot) => {
                slot.insert((OsString::from(key), OsString::from(value)));
            }
            Entry::Occupied(_) => {
                log::warn!("pty: environment entry '{key}' repeats an earlier name; ignored");
            }
        }
    }
    let dropped: Vec<Vec<u16>> = DROPPED_PARENT_ENV
        .iter()
        .map(|name| fold_name(OsStr::new(name)))
        .collect();
    for (key, value) in parent {
        let folded = fold_name(&key);
        if !merged.contains_key(&folded) && !dropped.contains(&folded) {
            merged.insert(folded, (key, value));
        }
    }

    let mut block = Vec::new();
    for (key, value) in merged.values() {
        block.extend(key.encode_wide());
        block.push(u16::from(b'='));
        block.extend(value.encode_wide());
        block.push(0);
    }
    if block.is_empty() {
        block.push(0);
    }
    block.push(0);
    block
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pair(key: &str, value: &str) -> (OsString, OsString) {
        (OsString::from(key), OsString::from(value))
    }

    fn custom(entries: &[(&str, &str)]) -> HashMap<String, String> {
        entries
            .iter()
            .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
            .collect()
    }

    /// The `key=value` strings of a block, in order.
    fn entries(block: &[u16]) -> Vec<String> {
        let body = block.strip_suffix(&[0]).unwrap_or(block);
        body.split(|unit| *unit == 0)
            .filter(|entry| !entry.is_empty())
            .map(String::from_utf16_lossy)
            .collect()
    }

    #[test]
    fn glyph_width_flags() {
        assert_eq!(glyph_flag(GlyphWidth::WcsWidth), 0x10);
        assert_eq!(glyph_flag(GlyphWidth::Graphemes), 0x08);
        assert_eq!(glyph_flag(GlyphWidth::default()), 0x10);
    }

    #[test]
    fn empty_block_is_double_nul() {
        assert_eq!(environment_block(&HashMap::new(), []), vec![0, 0]);
    }

    #[test]
    fn parent_is_copied() {
        let block = environment_block(&HashMap::new(), [pair("PATH", "p"), pair("HOME", "h")]);
        assert_eq!(entries(&block), ["HOME=h", "PATH=p"]);
        assert_eq!(&block[block.len() - 2..], [0, 0]);
    }

    #[test]
    fn case_insensitive_dedup_custom_wins() {
        let block = environment_block(&custom(&[("Path", "mine")]), [pair("PATH", "theirs")]);
        assert_eq!(entries(&block), ["Path=mine"]);
        // Two custom names that differ only in case: the sorted-first one wins, always.
        let block = environment_block(&custom(&[("a", "lower"), ("A", "upper")]), []);
        assert_eq!(entries(&block), ["A=upper"]);
    }

    #[test]
    fn identity_names_dropped_custom_kept() {
        let parent = [
            pair("wt_session", "x"),
            pair("TERM_PROGRAM", "y"),
            pair("KEEP", "z"),
        ];
        let block = environment_block(&custom(&[("TERM_PROGRAM", "mine")]), parent);
        assert_eq!(entries(&block), ["KEEP=z", "TERM_PROGRAM=mine"]);
    }

    #[test]
    fn block_is_sorted_case_insensitively() {
        let block = environment_block(
            &custom(&[("b", "2"), ("A", "1")]),
            [pair("=C:", "C:\\work")],
        );
        assert_eq!(entries(&block), ["=C:=C:\\work", "A=1", "b=2"]);
    }

    #[test]
    fn falls_back_to_system_without_dll() {
        let directory = std::env::temp_dir().join(format!("vt-pty-nodll-{}", std::process::id()));
        std::fs::create_dir_all(&directory).unwrap();
        let api = ConptyApi::resolve_in(&directory);
        assert_eq!(api.backend, ConptyBackend::System);
        std::fs::remove_dir_all(&directory).unwrap();
    }
}
