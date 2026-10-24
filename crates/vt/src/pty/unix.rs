//! Unix half of the pseudo-console: `openpty` plus `std::process::Command`.

#![expect(
    unsafe_code,
    reason = "libc FFI: openpty, fcntl, ioctl, termios, signal masks"
)]

use std::collections::HashMap;
use std::ffi::{CStr, CString, OsStr};
use std::fmt;
use std::fs::File;
use std::io::{self, Read};
use std::mem::MaybeUninit;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::net::UnixStream;
use std::os::unix::process::CommandExt;
use std::process::{Child, Command, Stdio};
use std::ptr;
use std::sync::{Arc, mpsc};
use std::thread;

use polling::{Event, PollMode, Poller};

use super::{
    ChildEvent, DROPPED_PARENT_ENV, EventedPty, EventedReadWrite, OnResize, Options,
    PTY_CHILD_EVENT_TOKEN, PTY_READ_WRITE_TOKEN, WindowSize, validate_env,
};

/// Signals whose handlers the embedder may have installed; the shell must start with defaults.
const RESET_SIGNALS: [libc::c_int; 6] = [
    libc::SIGCHLD,
    libc::SIGHUP,
    libc::SIGINT,
    libc::SIGQUIT,
    libc::SIGTERM,
    libc::SIGALRM,
];

/// A signal mask the child starts with ([`Options::child_signal_mask`]).
#[derive(Clone, Copy)]
pub struct SignalMask(libc::sigset_t);

impl fmt::Debug for SignalMask {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SignalMask")
    }
}

impl SignalMask {
    /// The signal mask of the calling thread.
    pub fn current() -> io::Result<Self> {
        // `sigemptyset` and `pthread_sigmask` may write only a prefix of the set, so every
        // byte is zeroed first.
        let mut set = MaybeUninit::<libc::sigset_t>::zeroed();
        // SAFETY: `set` is a live, writable `sigset_t`. `sigemptyset` initialises it; the
        // `pthread_sigmask` query (null new set) fills it with the current mask and returns an
        // error number instead of setting `errno`. `assume_init` runs only after both succeeded.
        unsafe {
            if libc::sigemptyset(set.as_mut_ptr()) != 0 {
                return Err(os_error("sigemptyset"));
            }
            let code = libc::pthread_sigmask(libc::SIG_SETMASK, ptr::null(), set.as_mut_ptr());
            if code != 0 {
                let cause = io::Error::from_raw_os_error(code);
                return Err(io::Error::new(
                    cause.kind(),
                    format!("pthread_sigmask failed: {cause}"),
                ));
            }
            Ok(Self(set.assume_init()))
        }
    }
}

/// A shell on the slave side of an `openpty` pair, polled through the master side.
pub struct PseudoConsole {
    /// Non-blocking, `FD_CLOEXEC`; reader and writer. Dropping it hangs the session up.
    master: File,
    /// Non-blocking; readable once the reaper thread saw the child exit.
    exit_signal: UnixStream,
    /// A second handle on the reaper's end of the pair. The reaper drops its own handle when it
    /// exits; without this one `exit_signal` would then read end-of-file forever and a level
    /// poll would spin. Never used.
    _wake_keepalive: UnixStream,
    exit_events: mpsc::Receiver<ChildEvent>,
    pid: u32,
}

impl PseudoConsole {
    /// Starts `options.shell` (default: `$SHELL`, else `/bin/sh`) on a new pseudo-terminal of
    /// `size`, as the leader of a new session with the terminal as its controlling terminal.
    pub fn spawn(options: &Options, size: WindowSize) -> io::Result<Self> {
        validate_env(&options.env)?;
        let (master, slave) = open_pty()?;
        set_utf8_input(&master);
        set_window_size(&master, size)?;

        let (program, args) = match &options.shell {
            Some(shell) => (shell.program().to_owned(), shell.args()),
            None => (default_shell(), &[][..]),
        };
        let mut command = Command::new(&program);
        command.args(args);
        command.stdin(dup_slave(&slave)?);
        command.stdout(dup_slave(&slave)?);
        command.stderr(dup_slave(&slave)?);
        apply_env(&mut command, &options.env);

        // Everything the closure needs is built before the fork: it may not allocate.
        let slave_fd = slave.as_raw_fd();
        let cwd = options
            .working_directory
            .as_deref()
            .and_then(|dir| CString::new(dir.as_os_str().as_bytes()).ok());
        let mask = options.child_signal_mask.map(|mask| mask.0);
        // SAFETY: the closure runs in the forked child before `exec`, where only
        // async-signal-safe calls are allowed. It makes only such calls (`setsid`, `chdir`,
        // `ioctl`, `sigprocmask`, `signal`), allocates nothing and takes no lock; it captures
        // an fd number, a prebuilt `CString` and a `Copy` mask. That also meets the contract of
        // `become_session_leader`: it is called post-fork, in the single-threaded child.
        unsafe {
            command
                .pre_exec(move || become_session_leader(slave_fd, cwd.as_deref(), mask.as_ref()));
        }
        // Everything fallible except the thread spawn is done before the fork, so a failure
        // never leaves an unreaped child behind. The child does not hold the master, so its
        // non-blocking flag is invisible to it.
        set_nonblocking(&master)?;
        let (exit_signal, waker) = UnixStream::pair().map_err(call_failed("socketpair"))?;
        exit_signal
            .set_nonblocking(true)
            .map_err(call_failed("set_nonblocking on the exit signal"))?;
        // The reaper may write after the console is gone: that must be an error, not SIGPIPE.
        suppress_sigpipe(&waker)?;
        let keepalive = waker.try_clone().map_err(call_failed("dup(waker)"))?;
        let (sender, exit_events) = mpsc::channel();
        let child = command.spawn().map_err(|cause| {
            io::Error::new(cause.kind(), format!("cannot start '{program}': {cause}"))
        })?;
        let pid = child.id();
        // Only the child holds the slave from here on.
        drop(command);
        drop(slave);
        reap_in_background(child, sender, waker)?;
        Ok(Self {
            master: File::from(master),
            exit_signal,
            _wake_keepalive: keepalive,
            exit_events,
            pid,
        })
    }

    /// The child's process id; always `Some` on Unix.
    pub fn child_pid(&self) -> Option<u32> {
        Some(self.pid)
    }
}

impl EventedReadWrite for PseudoConsole {
    type Reader = File;
    type Writer = File;

    unsafe fn register(
        &mut self,
        poller: &Arc<Poller>,
        interest: Event,
        mode: PollMode,
    ) -> io::Result<()> {
        let interest = master_interest(interest);
        // SAFETY: the caller of `register` promises to `deregister` both sources before
        // `self` or the poller is dropped. Both are fds owned by `self`.
        unsafe {
            poller.add_with_mode(&self.master, interest, mode)?;
            if let Err(cause) = poller.add_with_mode(
                &self.exit_signal,
                Event::readable(PTY_CHILD_EVENT_TOKEN),
                PollMode::Level,
            ) {
                // Leave nothing registered behind a failed call.
                let _ = poller.delete(&self.master);
                return Err(cause);
            }
        }
        Ok(())
    }

    fn reregister(
        &mut self,
        poller: &Arc<Poller>,
        interest: Event,
        mode: PollMode,
    ) -> io::Result<()> {
        poller.modify_with_mode(&self.master, master_interest(interest), mode)?;
        poller.modify_with_mode(
            &self.exit_signal,
            Event::readable(PTY_CHILD_EVENT_TOKEN),
            PollMode::Level,
        )
    }

    fn deregister(&mut self, poller: &Arc<Poller>) -> io::Result<()> {
        let master = poller.delete(&self.master);
        let exit_signal = poller.delete(&self.exit_signal);
        master.and(exit_signal)
    }

    fn reader(&mut self) -> &mut File {
        &mut self.master
    }

    fn writer(&mut self) -> &mut File {
        &mut self.master
    }
}

impl EventedPty for PseudoConsole {
    fn next_child_event(&mut self) -> Option<ChildEvent> {
        // Consume the wake byte first so a level-triggered poll stops reporting it. The reaper
        // sends the event before the byte, so a poll woken by the byte always finds the event.
        let mut byte = [0u8; 1];
        match self.exit_signal.read(&mut byte) {
            Ok(_) => {}
            Err(cause) if cause.kind() == io::ErrorKind::WouldBlock => {}
            Err(cause) => log::error!("pty: cannot read the exit signal: {cause}"),
        }
        self.exit_events.try_recv().ok()
    }
}

impl OnResize for PseudoConsole {
    fn on_resize(&mut self, size: WindowSize) -> io::Result<()> {
        set_window_size(&self.master, size)
    }
}

fn master_interest(mut interest: Event) -> Event {
    interest.key = PTY_READ_WRITE_TOKEN;
    interest
}

/// `"<call> failed: <os error>"` from the last OS error, keeping its kind.
fn os_error(call: &str) -> io::Error {
    let cause = io::Error::last_os_error();
    io::Error::new(cause.kind(), format!("{call} failed: {cause}"))
}

/// Wraps an I/O error as `"<call> failed: <cause>"`, keeping its kind.
fn call_failed(call: &'static str) -> impl FnOnce(io::Error) -> io::Error {
    move |cause| io::Error::new(cause.kind(), format!("{call} failed: {cause}"))
}

fn dup_slave(slave: &OwnedFd) -> io::Result<Stdio> {
    slave
        .try_clone()
        .map(Stdio::from)
        .map_err(call_failed("dup(slave)"))
}

/// Opens a master/slave pair, both `FD_CLOEXEC`: a shell started later must not inherit another
/// session's master, or dropping that master would never hang its session up.
fn open_pty() -> io::Result<(OwnedFd, OwnedFd)> {
    let mut master: libc::c_int = -1;
    let mut slave: libc::c_int = -1;
    // SAFETY: both out-parameters are live `c_int`s; the optional name, termios and winsize
    // arguments are null.
    let code = unsafe {
        libc::openpty(
            &mut master,
            &mut slave,
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null_mut(),
        )
    };
    if code != 0 {
        return Err(os_error("openpty"));
    }
    // SAFETY: `openpty` returned 0, so both fds are open and owned by nobody else.
    let (master, slave) = unsafe { (OwnedFd::from_raw_fd(master), OwnedFd::from_raw_fd(slave)) };
    set_cloexec(&master)?;
    set_cloexec(&slave)?;
    Ok((master, slave))
}

fn set_cloexec(fd: &OwnedFd) -> io::Result<()> {
    // SAFETY: `fd` is open for the whole call (borrowed from an `OwnedFd`).
    if unsafe { libc::fcntl(fd.as_raw_fd(), libc::F_SETFD, libc::FD_CLOEXEC) } == -1 {
        return Err(os_error("fcntl(F_SETFD)"));
    }
    Ok(())
}

fn set_nonblocking(fd: &OwnedFd) -> io::Result<()> {
    let raw = fd.as_raw_fd();
    // SAFETY: `raw` is open for both calls (borrowed from an `OwnedFd`).
    unsafe {
        let flags = libc::fcntl(raw, libc::F_GETFL);
        if flags == -1 {
            return Err(os_error("fcntl(F_GETFL)"));
        }
        if libc::fcntl(raw, libc::F_SETFL, flags | libc::O_NONBLOCK) == -1 {
            return Err(os_error("fcntl(F_SETFL)"));
        }
    }
    Ok(())
}

/// Marks the line discipline as UTF-8 so canonical-mode erase removes a whole character.
/// Best effort, and only where the flag exists.
fn set_utf8_input(master: &OwnedFd) {
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    {
        let mut attrs = MaybeUninit::<libc::termios>::uninit();
        // SAFETY: `master` is open for the calls; `assume_init` runs only after `tcgetattr`
        // returned 0 and filled `attrs`.
        unsafe {
            if libc::tcgetattr(master.as_raw_fd(), attrs.as_mut_ptr()) != 0 {
                return;
            }
            let mut attrs = attrs.assume_init();
            attrs.c_iflag |= libc::IUTF8;
            libc::tcsetattr(master.as_raw_fd(), libc::TCSANOW, &attrs);
        }
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    let _ = master;
}

fn winsize(size: WindowSize) -> libc::winsize {
    libc::winsize {
        ws_row: size.rows,
        ws_col: size.cols,
        ws_xpixel: size.cols.saturating_mul(size.cell_width),
        ws_ypixel: size.rows.saturating_mul(size.cell_height),
    }
}

fn set_window_size(master: &impl AsRawFd, size: WindowSize) -> io::Result<()> {
    let ws = winsize(size);
    // SAFETY: the master is open for the call (borrowed); `ws` lives across it.
    let code = unsafe { libc::ioctl(master.as_raw_fd(), libc::TIOCSWINSZ as _, &ws) };
    if code == -1 {
        let cause = io::Error::last_os_error();
        return Err(io::Error::other(format!(
            "ioctl TIOCSWINSZ to {}x{} failed: {cause}",
            size.cols, size.rows
        )));
    }
    Ok(())
}

fn default_shell() -> String {
    std::env::var("SHELL")
        .ok()
        .filter(|shell| !shell.is_empty())
        .unwrap_or_else(|| "/bin/sh".to_owned())
}

/// The parent's environment minus the launching terminal's identity, then `custom` on top.
fn apply_env(command: &mut Command, custom: &HashMap<String, String>) {
    for name in DROPPED_PARENT_ENV {
        command.env_remove(name);
    }
    for (key, value) in custom {
        command.env(OsStr::new(key), OsStr::new(value));
    }
}

/// The child side, between std's `dup2` of stdio and `exec`. Async-signal-safe calls only.
///
/// # Safety
///
/// Must run post-fork in the single-threaded child, before `exec`, with `slave_fd` still open.
unsafe fn become_session_leader(
    slave_fd: RawFd,
    cwd: Option<&CStr>,
    mask: Option<&libc::sigset_t>,
) -> io::Result<()> {
    // SAFETY: the caller guarantees a post-fork, single-threaded child; every call below is
    // async-signal-safe. `slave_fd` is still open (its `FD_CLOEXEC` closes it only at `exec`),
    // `cwd` is NUL-terminated and `mask` was initialised by `SignalMask::current`.
    unsafe {
        if libc::setsid() == -1 {
            return Err(io::Error::last_os_error());
        }
        if let Some(dir) = cwd {
            // Best effort: an invalid directory leaves the inherited one.
            libc::chdir(dir.as_ptr());
        }
        if libc::ioctl(slave_fd, libc::TIOCSCTTY as _, 0) == -1 {
            return Err(io::Error::last_os_error());
        }
        if let Some(mask) = mask
            && libc::sigprocmask(libc::SIG_SETMASK, mask, ptr::null_mut()) == -1
        {
            return Err(io::Error::last_os_error());
        }
        for signal in RESET_SIGNALS {
            libc::signal(signal, libc::SIG_DFL);
        }
    }
    Ok(())
}

/// Waits for the child on its own thread, so no `SIGCHLD` handler (process-global) and no
/// timer is needed. The event is queued before the wake byte is written. The thread outlives
/// the console until the child exits; it is not joined.
fn reap_in_background(
    mut child: Child,
    events: mpsc::Sender<ChildEvent>,
    waker: UnixStream,
) -> io::Result<()> {
    thread::Builder::new()
        .name("oneterm-vt-pty-reaper".to_owned())
        .spawn(move || {
            let status = child.wait().ok();
            // The console may be gone already: both results are ignored.
            let _ = events.send(ChildEvent::Exited(status));
            send_wake_byte(&waker);
        })
        .map(|_| ())
        .map_err(|cause| io::Error::new(cause.kind(), format!("reaper thread failed: {cause}")))
}

/// Writes the wake byte without raising `SIGPIPE` when the console is already gone.
#[cfg(any(
    target_os = "linux",
    target_os = "android",
    target_os = "freebsd",
    target_os = "netbsd",
    target_os = "openbsd",
    target_os = "illumos"
))]
fn send_wake_byte(waker: &UnixStream) {
    let byte = [1u8];
    // SAFETY: `waker` is open for the call and `byte` is a live one-byte buffer. `MSG_NOSIGNAL`
    // turns a closed peer into `EPIPE`, which is ignored on purpose: nobody is left to wake.
    unsafe {
        libc::send(
            waker.as_raw_fd(),
            byte.as_ptr().cast(),
            byte.len(),
            libc::MSG_NOSIGNAL,
        );
    }
}

/// Other Unixes have no `MSG_NOSIGNAL`; Apple's `SO_NOSIGPIPE` is set in [`suppress_sigpipe`].
#[cfg(not(any(
    target_os = "linux",
    target_os = "android",
    target_os = "freebsd",
    target_os = "netbsd",
    target_os = "openbsd",
    target_os = "illumos"
)))]
fn send_wake_byte(mut waker: &UnixStream) {
    // The console may be gone already: the result is ignored.
    let _ = io::Write::write_all(&mut waker, &[1]);
}

/// Sets `SO_NOSIGPIPE` on Apple, where `send` has no `MSG_NOSIGNAL`; the option is per socket, so
/// it covers a duplicate handle too. A no-op elsewhere.
fn suppress_sigpipe(waker: &UnixStream) -> io::Result<()> {
    #[cfg(target_vendor = "apple")]
    {
        let on: libc::c_int = 1;
        // SAFETY: `waker` is open for the call; the option value is a live `c_int` and its
        // length is passed as `size_of::<c_int>()`.
        let code = unsafe {
            libc::setsockopt(
                waker.as_raw_fd(),
                libc::SOL_SOCKET,
                libc::SO_NOSIGPIPE,
                (&raw const on).cast(),
                size_of::<libc::c_int>() as libc::socklen_t,
            )
        };
        if code == -1 {
            return Err(os_error("setsockopt(SO_NOSIGPIPE)"));
        }
    }
    #[cfg(not(target_vendor = "apple"))]
    let _ = waker;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::io::Write;
    use std::time::{Duration, Instant};

    use super::*;
    use crate::pty::Shell;

    const WAIT: Duration = Duration::from_secs(10);

    fn size() -> WindowSize {
        WindowSize {
            rows: 24,
            cols: 80,
            cell_width: 8,
            cell_height: 16,
        }
    }

    fn sh(script: &str) -> PseudoConsole {
        let options = Options {
            shell: Some(Shell::new(
                "/bin/sh".to_owned(),
                vec!["-c".to_owned(), script.to_owned()],
            )),
            ..Options::default()
        };
        PseudoConsole::spawn(&options, size()).unwrap()
    }

    /// Reads until `done(output)` or the output ends; polls without a `Poller`.
    fn read_until(pty: &mut PseudoConsole, done: impl Fn(&str) -> bool) -> String {
        let deadline = Instant::now() + WAIT;
        let mut output = Vec::new();
        let mut buf = [0u8; 1024];
        while Instant::now() < deadline {
            match pty.reader().read(&mut buf) {
                Ok(0) => break,
                Ok(n) => output.extend_from_slice(&buf[..n]),
                Err(cause) if cause.kind() == io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(10));
                }
                Err(_) => break, // EIO on Linux: output ended
            }
            if done(&String::from_utf8_lossy(&output)) {
                break;
            }
        }
        String::from_utf8_lossy(&output).into_owned()
    }

    fn wait_for_exit(pty: &mut PseudoConsole) -> ChildEvent {
        let deadline = Instant::now() + WAIT;
        while Instant::now() < deadline {
            if let Some(event) = pty.next_child_event() {
                return event;
            }
            thread::sleep(Duration::from_millis(10));
        }
        panic!("the child did not exit in time");
    }

    #[test]
    fn identity_env_is_dropped_and_custom_wins() {
        let mut command = Command::new("true");
        let custom = HashMap::from([
            ("TERM_PROGRAM".to_owned(), "mine".to_owned()),
            ("TERM".to_owned(), "xterm-256color".to_owned()),
        ]);
        apply_env(&mut command, &custom);
        let envs: HashMap<_, _> = command.get_envs().collect();
        assert_eq!(envs.get(OsStr::new("WT_SESSION")), Some(&None));
        assert_eq!(envs.get(OsStr::new("ITERM_SESSION_ID")), Some(&None));
        assert_eq!(
            envs.get(OsStr::new("TERM_PROGRAM")),
            Some(&Some(OsStr::new("mine")))
        );
        assert_eq!(
            envs.get(OsStr::new("TERM")),
            Some(&Some(OsStr::new("xterm-256color")))
        );
    }

    #[test]
    fn master_and_slave_are_cloexec() {
        let (master, slave) = open_pty().unwrap();
        for fd in [&master, &slave] {
            // SAFETY: the fd is open for the call.
            let flags = unsafe { libc::fcntl(fd.as_raw_fd(), libc::F_GETFD) };
            assert_ne!(flags & libc::FD_CLOEXEC, 0);
        }
        let pty = sh("exit 0");
        // SAFETY: the master is open for the call.
        let flags = unsafe { libc::fcntl(pty.master.as_raw_fd(), libc::F_GETFD) };
        assert_ne!(flags & libc::FD_CLOEXEC, 0);
    }

    #[test]
    fn winsize_pixels_saturate() {
        let ws = winsize(WindowSize {
            rows: 30,
            cols: 100,
            cell_width: 10,
            cell_height: 20,
        });
        assert_eq!((ws.ws_row, ws.ws_col), (30, 100));
        assert_eq!((ws.ws_xpixel, ws.ws_ypixel), (1000, 600));
        let ws = winsize(WindowSize {
            rows: 60000,
            cols: 60000,
            cell_width: 60000,
            cell_height: 60000,
        });
        assert_eq!((ws.ws_xpixel, ws.ws_ypixel), (u16::MAX, u16::MAX));
    }

    #[test]
    fn signal_mask_current_succeeds() {
        let mask = SignalMask::current().unwrap();
        assert_eq!(format!("{mask:?}"), "SignalMask");
    }

    #[test]
    fn output_reaches_the_reader() {
        let mut pty = sh("printf hi");
        let output = read_until(&mut pty, |text| text.contains("hi"));
        assert!(output.contains("hi"), "{output:?}");
    }

    #[test]
    fn input_reaches_the_child() {
        let options = Options {
            shell: Some(Shell::new("/bin/cat".to_owned(), Vec::new())),
            ..Options::default()
        };
        let mut pty = PseudoConsole::spawn(&options, size()).unwrap();
        pty.writer().write_all(b"ping\n").unwrap();
        // The line discipline echoes once and `cat` answers once.
        let output = read_until(&mut pty, |text| text.matches("ping").count() >= 2);
        assert!(output.matches("ping").count() >= 2, "{output:?}");
    }

    #[test]
    fn exit_code_is_reported_once() {
        let mut pty = sh("exit 3");
        let ChildEvent::Exited(status) = wait_for_exit(&mut pty);
        assert_eq!(status.and_then(|status| status.code()), Some(3));
        assert_eq!(pty.next_child_event(), None);
    }

    #[test]
    fn exit_signal_stays_quiet_after_the_wake_byte() {
        let mut pty = sh("exit 0");
        wait_for_exit(&mut pty);
        // The reaper queues the event before the byte, and drops its end right after: give both
        // time, then expect `WouldBlock` (a level poll would spin on end-of-file).
        let deadline = Instant::now() + Duration::from_millis(300);
        let mut byte = [0u8; 1];
        while Instant::now() < deadline {
            match pty.exit_signal.read(&mut byte) {
                Ok(1) => {}
                Ok(count) => panic!("exit signal read {count} bytes: end-of-file"),
                Err(cause) => assert_eq!(cause.kind(), io::ErrorKind::WouldBlock),
            }
            thread::sleep(Duration::from_millis(10));
        }
    }

    #[test]
    fn child_pid_is_positive() {
        let pty = sh("exit 0");
        assert!(pty.child_pid().is_some_and(|pid| pid > 0));
    }

    #[test]
    fn resize_of_a_live_pty_succeeds() {
        let mut pty = sh("sleep 1");
        pty.on_resize(WindowSize {
            rows: 30,
            cols: 100,
            cell_width: 9,
            cell_height: 18,
        })
        .unwrap();
    }

    #[test]
    fn rejected_resize_is_an_error() {
        let mut pty = sh("sleep 1");
        match pty.on_resize(WindowSize::default()) {
            Ok(()) => {}
            Err(cause) => assert!(cause.to_string().contains("TIOCSWINSZ"), "{cause}"),
        }
    }

    #[test]
    fn missing_program_names_it() {
        let options = Options {
            shell: Some(Shell::new(
                "/nonexistent/vt-pty-shell".to_owned(),
                Vec::new(),
            )),
            ..Options::default()
        };
        let cause = PseudoConsole::spawn(&options, size()).err().unwrap();
        assert!(
            cause
                .to_string()
                .starts_with("cannot start '/nonexistent/vt-pty-shell': "),
            "{cause}"
        );
    }
}
