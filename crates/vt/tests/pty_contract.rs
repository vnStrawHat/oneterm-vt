//! The public `pty` contract, driven the way an embedder would: a real shell, a real `Poller`,
//! the two tokens, nothing but the public API.

#![cfg(feature = "pty")]
#![expect(unsafe_code, reason = "registers a PseudoConsole with a Poller")]

use std::io::{self, Read, Write};
use std::process::Command;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use oneterm_vt::pty::{
    ChildEvent, EventedPty, EventedReadWrite, OnResize, Options, PTY_CHILD_EVENT_TOKEN,
    PTY_READ_WRITE_TOKEN, PseudoConsole, Shell, WindowSize,
};
use polling::{Event, Events, PollMode, Poller};

#[cfg(unix)]
const WAIT: Duration = Duration::from_secs(10);
#[cfg(windows)]
const WAIT: Duration = Duration::from_secs(15);

const SIZE: WindowSize = WindowSize {
    rows: 24,
    cols: 80,
    cell_width: 8,
    cell_height: 16,
};

/// Runs `command` in the platform shell and exits.
fn script(command: &str) -> Options {
    #[cfg(unix)]
    let shell = Shell::new(
        "/bin/sh".to_owned(),
        vec!["-c".to_owned(), command.to_owned()],
    );
    #[cfg(windows)]
    let shell = Shell::new(
        "cmd.exe".to_owned(),
        vec!["/d".to_owned(), "/c".to_owned(), command.to_owned()],
    );
    Options {
        shell: Some(shell),
        ..Options::default()
    }
}

/// A program that stays alive and reads what is typed.
fn interactive() -> Options {
    #[cfg(unix)]
    let shell = Shell::new("/bin/cat".to_owned(), Vec::new());
    #[cfg(windows)]
    let shell = Shell::new(
        "cmd.exe".to_owned(),
        ["/d", "/q", "/k", "echo off"].map(str::to_owned).to_vec(),
    );
    Options {
        shell: Some(shell),
        ..Options::default()
    }
}

/// Input that makes `interactive()` end.
#[cfg(unix)]
const QUIT: &[u8] = b"\x04";
#[cfg(windows)]
const QUIT: &[u8] = b"exit\r\n";

fn is_output_ended(error: &io::Error) -> bool {
    #[cfg(unix)]
    {
        error.raw_os_error() == Some(libc::EIO)
    }
    #[cfg(not(unix))]
    {
        let _ = error;
        false
    }
}

/// A console registered on its own poller, with the output and the exit collected by `pump`.
struct Session {
    pty: PseudoConsole,
    poller: Arc<Poller>,
    output: Vec<u8>,
    /// The reader reported the end of the output: stop reading and wait for the child token.
    output_ended: bool,
    exit: Option<ChildEvent>,
}

impl Session {
    fn start(options: &Options) -> io::Result<Self> {
        let mut pty = PseudoConsole::spawn(options, SIZE)?;
        let poller = Arc::new(Poller::new()?);
        // SAFETY: `Session::drop` deregisters before the console and the poller are dropped.
        // The key 0 is replaced by `PTY_READ_WRITE_TOKEN`, as the API promises.
        unsafe { pty.register(&poller, Event::readable(0), PollMode::Level)? };
        Ok(Self {
            pty,
            poller,
            output: Vec::new(),
            output_ended: false,
            exit: None,
        })
    }

    fn text(&self) -> String {
        String::from_utf8_lossy(&self.output).into_owned()
    }

    fn pid(&self) -> io::Result<u32> {
        self.pty
            .child_pid()
            .ok_or_else(|| io::Error::other("the child has no pid"))
    }

    fn write(&mut self, bytes: &[u8]) -> io::Result<()> {
        self.pty.writer().write_all(bytes)
    }

    /// Reads until `WouldBlock` or the end of output, as the reader contract says.
    fn drain(&mut self) -> io::Result<()> {
        if self.output_ended {
            return Ok(());
        }
        let mut buf = [0u8; 4096];
        loop {
            match self.pty.reader().read(&mut buf) {
                Ok(0) => break,
                Ok(n) => self.output.extend_from_slice(&buf[..n]),
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => return Ok(()),
                Err(error) if is_output_ended(&error) => break,
                Err(error) => return Err(error),
            }
        }
        // An ended master keeps waking a level poll on Linux; the exit event follows.
        self.output_ended = true;
        Ok(())
    }

    /// Polls until `until` holds or `timeout` passes; returns whether it held.
    fn pump(&mut self, until: impl Fn(&Self) -> bool, timeout: Duration) -> io::Result<bool> {
        let deadline = Instant::now() + timeout;
        let mut events = Events::new();
        loop {
            if until(self) {
                return Ok(true);
            }
            let Some(left) = deadline.checked_duration_since(Instant::now()) else {
                return Ok(false);
            };
            events.clear();
            let slice = left.min(Duration::from_millis(100));
            self.poller.wait(&mut events, Some(slice))?;
            for event in events.iter() {
                match event.key {
                    PTY_READ_WRITE_TOKEN => self.drain()?,
                    PTY_CHILD_EVENT_TOKEN => {
                        if let Some(exit) = self.pty.next_child_event() {
                            self.exit = Some(exit);
                        }
                    }
                    other => {
                        return Err(io::Error::other(format!("unexpected poll key {other}")));
                    }
                }
            }
        }
    }

    fn pump_for(&mut self, marker: &str) -> io::Result<bool> {
        self.pump(|session| session.text().contains(marker), WAIT)
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        let _ = self.pty.deregister(&self.poller);
    }
}

#[cfg(unix)]
fn alive(pid: u32) -> io::Result<bool> {
    use std::process::Stdio;

    let status = Command::new("kill")
        .args(["-0", &pid.to_string()])
        .stderr(Stdio::null())
        .status()?;
    Ok(status.success())
}

#[cfg(windows)]
fn alive(pid: u32) -> io::Result<bool> {
    let listing = Command::new("tasklist")
        .args(["/FI", &format!("PID eq {pid}"), "/NH", "/FO", "CSV"])
        .output()?;
    Ok(String::from_utf8_lossy(&listing.stdout).contains(&format!("\"{pid}\"")))
}

fn dies_within(pid: u32, timeout: Duration) -> io::Result<bool> {
    let deadline = Instant::now() + timeout;
    while alive(pid)? {
        if Instant::now() > deadline {
            return Ok(false);
        }
        thread::sleep(Duration::from_millis(50));
    }
    Ok(true)
}

#[test]
fn echo_round_trip() -> io::Result<()> {
    let mut session = Session::start(&script("echo vt-pty-contract"))?;
    assert!(session.pump_for("vt-pty-contract")?, "{:?}", session.text());
    Ok(())
}

#[test]
fn typed_input_reaches_the_child() -> io::Result<()> {
    let mut session = Session::start(&interactive())?;
    // Unix: the line discipline echoes the line and `cat` prints it again.
    // Windows: `^e` is an escaped `e`, so only the command's output spells the marker.
    #[cfg(unix)]
    let (typed, seen) = (&b"vt-pty-typed\n"[..], 2);
    #[cfg(windows)]
    let (typed, seen) = (&b"echo vt-pty-typ^ed\r\n"[..], 1);
    session.write(typed)?;
    let arrived = session.pump(
        |session| session.text().matches("vt-pty-typed").count() >= seen,
        WAIT,
    )?;
    assert!(arrived, "{:?}", session.text());
    Ok(())
}

#[test]
fn exit_code_arrives_on_child_token() -> io::Result<()> {
    // `Session` deregisters on drop, so an assertion failure cannot leave the sources behind.
    let mut session = Session::start(&script("exit 3"))?;
    // Only the child token is looked at and nothing is read: the exit needs no output.
    let deadline = Instant::now() + WAIT;
    let mut events = Events::new();
    let mut woke_on_child_token = false;
    while !woke_on_child_token && Instant::now() < deadline {
        events.clear();
        session
            .poller
            .wait(&mut events, Some(Duration::from_millis(100)))?;
        woke_on_child_token = events.iter().any(|e| e.key == PTY_CHILD_EVENT_TOKEN);
    }
    assert!(woke_on_child_token, "no wake on the child token");
    let Some(ChildEvent::Exited(status)) = session.pty.next_child_event() else {
        return Err(io::Error::other("no exit event after the wake"));
    };
    assert_eq!(status.and_then(|status| status.code()), Some(3));
    assert_eq!(session.pty.next_child_event(), None);
    Ok(())
}

#[test]
fn env_reaches_the_child() -> io::Result<()> {
    #[cfg(unix)]
    let command = "echo $VT_PTY_PROBE";
    #[cfg(windows)]
    let command = "echo %VT_PTY_PROBE%";
    let mut options = script(command);
    options
        .env
        .insert("VT_PTY_PROBE".to_owned(), "probe-42".to_owned());
    let mut session = Session::start(&options)?;
    assert!(session.pump_for("probe-42")?, "{:?}", session.text());
    Ok(())
}

#[test]
fn cwd_applies() -> io::Result<()> {
    let name = format!("vt-pty-cwd-{}", std::process::id());
    let directory = std::env::temp_dir().join(&name);
    std::fs::create_dir_all(&directory)?;
    #[cfg(unix)]
    let command = "pwd";
    #[cfg(windows)]
    let command = "cd";
    let mut options = script(command);
    options.working_directory = Some(directory.clone());
    let mut session = Session::start(&options)?;
    let seen = session.pump_for(&name)?;
    let text = session.text();
    drop(session);
    let _ = std::fs::remove_dir(&directory);
    assert!(seen, "{text:?}");
    Ok(())
}

#[test]
fn resize_on_both_sides_of_exit() -> io::Result<()> {
    let mut session = Session::start(&interactive())?;
    let bigger = WindowSize {
        rows: 30,
        cols: 100,
        cell_width: 9,
        cell_height: 18,
    };
    session.pty.on_resize(bigger)?;
    session.write(QUIT)?;
    assert!(
        session.pump(|session| session.exit.is_some(), WAIT)?,
        "the child did not exit"
    );
    // After the exit a resize may succeed or fail; it must not panic.
    let _ = session.pty.on_resize(SIZE);
    Ok(())
}

#[cfg(unix)]
#[test]
fn child_sees_the_new_size() -> io::Result<()> {
    let mut session = Session::start(&script("read x; stty size"))?;
    session.pty.on_resize(WindowSize {
        rows: 30,
        cols: 100,
        ..WindowSize::default()
    })?;
    session.write(b"\n")?;
    assert!(session.pump_for("30 100")?, "{:?}", session.text());
    Ok(())
}

#[cfg(unix)]
#[test]
fn drop_ends_the_child() -> io::Result<()> {
    let session = Session::start(&script("sleep 30"))?;
    let pid = session.pid()?;
    assert!(alive(pid)?);
    drop(session);
    assert!(
        dies_within(pid, Duration::from_secs(5))?,
        "the child outlived its console"
    );
    Ok(())
}

#[cfg(windows)]
#[test]
fn drop_ends_the_child() -> io::Result<()> {
    let mut session = Session::start(&script("ping -n 30 127.0.0.1"))?;
    assert!(
        session.pump(|session| !session.output.is_empty(), WAIT)?,
        "no output before the drop"
    );
    let pid = session.pid()?;
    assert!(alive(pid)?);
    let started = Instant::now();
    drop(session);
    assert!(
        started.elapsed() < Duration::from_secs(4),
        "drop took {:?}",
        started.elapsed()
    );
    assert!(
        dies_within(pid, Duration::from_secs(5))?,
        "the child outlived its console"
    );
    Ok(())
}

#[cfg(unix)]
#[test]
fn drop_ends_only_its_own_child() -> io::Result<()> {
    // B is started while A's master is open in this process: without close-on-exec, B's shell
    // would hold A's master and dropping A would never hang A up.
    let first = Session::start(&script("sleep 30"))?;
    let second = Session::start(&script("sleep 30"))?;
    let (first_pid, second_pid) = (first.pid()?, second.pid()?);
    drop(first);
    assert!(
        dies_within(first_pid, Duration::from_secs(5))?,
        "A outlived its console"
    );
    assert!(alive(second_pid)?, "dropping A ended B");
    drop(second);
    assert!(dies_within(second_pid, Duration::from_secs(5))?);
    Ok(())
}
