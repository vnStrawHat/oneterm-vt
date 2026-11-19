//! The three traits against a fake that needs no child: two TCP socket pairs stand in for the
//! master side and the child-exit signal, so the contract is checked on every platform. The
//! same fake behind a [`Registered`] checks the safe registration.

#![expect(unsafe_code, reason = "implements and calls register")]

use std::io::{self, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::process::ExitStatus;
use std::sync::{Arc, Mutex, mpsc};
use std::time::Duration;

#[cfg(unix)]
use std::os::unix::process::ExitStatusExt;
#[cfg(windows)]
use std::os::windows::process::ExitStatusExt;

use polling::{Event, Events, PollMode, Poller};

use super::{
    ChildEvent, EventedPty, EventedReadWrite, OnResize, PTY_CHILD_EVENT_TOKEN,
    PTY_READ_WRITE_TOKEN, Registered, WindowSize,
};

const WAIT: Duration = Duration::from_secs(5);

/// The embedder's half: what a pseudo-console would expose.
struct Loopback {
    io: TcpStream,
    child_signal: TcpStream,
    exit_events: mpsc::Receiver<ChildEvent>,
    sizes: Arc<Mutex<Vec<WindowSize>>>,
    /// What happened to the registration and to this value, in order.
    lifecycle: Arc<Mutex<Vec<&'static str>>>,
    /// `register` fails after it added the I/O source, as a poller out of room would.
    refuses_the_child_signal: bool,
}

/// The "child" half: writes output, reads input, reports exit.
struct Remote {
    io: TcpStream,
    child_signal: TcpStream,
    exit_events: mpsc::Sender<ChildEvent>,
    sizes: Arc<Mutex<Vec<WindowSize>>>,
    lifecycle: Arc<Mutex<Vec<&'static str>>>,
}

fn socket_pair() -> (TcpStream, TcpStream) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let ours = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
    let (theirs, _) = listener.accept().unwrap();
    ours.set_nonblocking(true).unwrap();
    (ours, theirs)
}

fn loopback() -> (Loopback, Remote) {
    let (io, remote_io) = socket_pair();
    let (child_signal, remote_signal) = socket_pair();
    let (events_tx, exit_events) = mpsc::channel();
    let sizes = Arc::new(Mutex::new(Vec::new()));
    let lifecycle = Arc::new(Mutex::new(Vec::new()));
    (
        Loopback {
            io,
            child_signal,
            exit_events,
            sizes: Arc::clone(&sizes),
            lifecycle: Arc::clone(&lifecycle),
            refuses_the_child_signal: false,
        },
        Remote {
            io: remote_io,
            child_signal: remote_signal,
            exit_events: events_tx,
            sizes,
            lifecycle,
        },
    )
}

impl Remote {
    fn exit(&mut self, status: Option<ExitStatus>) {
        self.exit_events.send(ChildEvent::Exited(status)).unwrap();
        self.child_signal.write_all(&[1]).unwrap();
    }
}

impl EventedReadWrite for Loopback {
    type Reader = TcpStream;
    type Writer = TcpStream;

    unsafe fn register(
        &mut self,
        poller: &Arc<Poller>,
        mut interest: Event,
        mode: PollMode,
    ) -> io::Result<()> {
        interest.key = PTY_READ_WRITE_TOKEN;
        // SAFETY: the tests deregister before the poller and the sockets are dropped.
        unsafe {
            poller.add_with_mode(&self.io, interest, mode)?;
            if self.refuses_the_child_signal {
                return Err(io::Error::other("the child signal was refused"));
            }
            poller.add_with_mode(
                &self.child_signal,
                Event::readable(PTY_CHILD_EVENT_TOKEN),
                PollMode::Level,
            )?;
        }
        self.lifecycle.lock().unwrap().push("registered");
        Ok(())
    }

    fn reregister(
        &mut self,
        poller: &Arc<Poller>,
        mut interest: Event,
        mode: PollMode,
    ) -> io::Result<()> {
        interest.key = PTY_READ_WRITE_TOKEN;
        poller.modify_with_mode(&self.io, interest, mode)?;
        poller.modify_with_mode(
            &self.child_signal,
            Event::readable(PTY_CHILD_EVENT_TOKEN),
            PollMode::Level,
        )
    }

    fn deregister(&mut self, poller: &Arc<Poller>) -> io::Result<()> {
        self.lifecycle.lock().unwrap().push("deregistered");
        poller.delete(&self.io)?;
        poller.delete(&self.child_signal)
    }

    fn reader(&mut self) -> &mut TcpStream {
        &mut self.io
    }

    fn writer(&mut self) -> &mut TcpStream {
        &mut self.io
    }
}

impl EventedPty for Loopback {
    fn next_child_event(&mut self) -> Option<ChildEvent> {
        let mut byte = [0u8; 1];
        let _ = self.child_signal.read(&mut byte);
        self.exit_events.try_recv().ok()
    }
}

impl Drop for Loopback {
    fn drop(&mut self) {
        self.lifecycle.lock().unwrap().push("dropped");
    }
}

impl OnResize for Loopback {
    fn on_resize(&mut self, size: WindowSize) -> io::Result<()> {
        self.sizes.lock().unwrap().push(size);
        Ok(())
    }
}

/// Waits for the next event.
fn next_event(poller: &Poller) -> Event {
    let mut events = Events::new();
    assert!(
        poller.wait(&mut events, Some(WAIT)).unwrap() > 0,
        "no event in time"
    );
    events.iter().next().unwrap()
}

/// Waits for the next event and returns its key.
fn next_key(poller: &Poller) -> usize {
    next_event(poller).key
}

fn registered(pty: Loopback) -> io::Result<Registered<Loopback>> {
    let poller = Arc::new(Poller::new().unwrap());
    Registered::new(pty, poller, Event::readable(0), PollMode::Level)
}

/// Everything below goes through the traits only, as an embedder's generic code would.
fn exercise<P: EventedPty + OnResize>(pty: &mut P, remote: &mut Remote) {
    let poller = Arc::new(Poller::new().unwrap());
    // SAFETY: deregistered at the end of this function, before `poller` drops.
    unsafe {
        pty.register(&poller, Event::readable(0), PollMode::Level)
            .unwrap();
    }

    // Output arrives under the read/write token, whatever key the caller asked for.
    remote.io.write_all(b"hello").unwrap();
    assert_eq!(next_key(&poller), PTY_READ_WRITE_TOKEN);
    let mut buf = [0u8; 16];
    let n = pty.reader().read(&mut buf).unwrap();
    assert_eq!(&buf[..n], b"hello");
    assert_eq!(
        pty.reader().read(&mut buf).unwrap_err().kind(),
        io::ErrorKind::WouldBlock
    );

    // Input reaches the "child".
    pty.writer().write_all(b"typed").unwrap();
    let mut seen = [0u8; 5];
    remote.io.read_exact(&mut seen).unwrap();
    assert_eq!(&seen, b"typed");

    // Resize is recorded.
    let size = WindowSize {
        rows: 30,
        cols: 100,
        cell_width: 9,
        cell_height: 18,
    };
    pty.on_resize(size).unwrap();
    assert_eq!(*remote.sizes.lock().unwrap(), [size]);

    // Exit arrives on the child token, exactly once.
    remote.exit(Some(ExitStatus::from_raw(0)));
    assert_eq!(next_key(&poller), PTY_CHILD_EVENT_TOKEN);
    assert_eq!(
        pty.next_child_event(),
        Some(ChildEvent::Exited(Some(ExitStatus::from_raw(0))))
    );
    assert_eq!(pty.next_child_event(), None);

    pty.deregister(&poller).unwrap();
}

#[test]
fn loopback_implements_the_evented_contract() {
    let (mut pty, mut remote) = loopback();
    exercise(&mut pty, &mut remote);
}

#[test]
fn an_exit_without_a_code_is_still_an_exit() {
    let (mut pty, mut remote) = loopback();
    remote.exit(None);
    assert_eq!(pty.next_child_event(), Some(ChildEvent::Exited(None)));
    assert_eq!(pty.next_child_event(), None);
}

/// The registration holds the only `Arc` of the poller here: it is what keeps the poller alive.
#[test]
fn a_registration_reads_writes_resizes_and_reports_the_exit() {
    let (pty, mut remote) = loopback();
    let mut pty = registered(pty).unwrap();

    remote.io.write_all(b"hello").unwrap();
    assert_eq!(next_key(pty.poller()), PTY_READ_WRITE_TOKEN);
    let mut buf = [0u8; 16];
    let n = pty.read(&mut buf).unwrap();
    assert_eq!(&buf[..n], b"hello");
    assert_eq!(
        pty.read(&mut buf).unwrap_err().kind(),
        io::ErrorKind::WouldBlock
    );

    pty.write_all(b"typed").unwrap();
    pty.flush().unwrap();
    let mut seen = [0u8; 5];
    remote.io.read_exact(&mut seen).unwrap();
    assert_eq!(&seen, b"typed");

    let size = WindowSize {
        rows: 30,
        cols: 100,
        cell_width: 9,
        cell_height: 18,
    };
    pty.on_resize(size).unwrap();
    assert_eq!(*remote.sizes.lock().unwrap(), [size]);

    remote.exit(Some(ExitStatus::from_raw(0)));
    assert_eq!(next_key(pty.poller()), PTY_CHILD_EVENT_TOKEN);
    assert_eq!(
        pty.next_child_event(),
        Some(ChildEvent::Exited(Some(ExitStatus::from_raw(0))))
    );
    assert_eq!(pty.next_child_event(), None);
}

#[test]
fn a_registration_changes_its_interest() {
    let (pty, _remote) = loopback();
    let mut pty = registered(pty).unwrap();
    // Nothing to read: a readable interest stays quiet, a writable one wakes at once.
    pty.reregister(Event::writable(0), PollMode::Level).unwrap();
    let event = next_event(pty.poller());
    assert_eq!(event.key, PTY_READ_WRITE_TOKEN);
    assert!(event.writable);
}

#[test]
fn a_dropped_registration_deregisters_before_its_console_is_dropped() {
    let (pty, remote) = loopback();
    let pty = registered(pty).unwrap();
    // A clone of the poller may outlive the registration.
    let poller = Arc::clone(pty.poller());
    drop(pty);
    assert_eq!(
        *remote.lifecycle.lock().unwrap(),
        ["registered", "deregistered", "dropped"]
    );
    drop(poller);
}

#[test]
fn a_failed_registration_takes_back_what_it_added() {
    let (mut pty, remote) = loopback();
    pty.refuses_the_child_signal = true;
    let cause = registered(pty).err().unwrap();
    assert_eq!(cause.to_string(), "the child signal was refused");
    assert_eq!(
        *remote.lifecycle.lock().unwrap(),
        ["deregistered", "dropped"]
    );
}
