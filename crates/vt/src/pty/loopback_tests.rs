//! The three traits against a fake that needs no child: two TCP socket pairs stand in for the
//! master side and the child-exit signal, so the contract is checked on every platform.

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
    PTY_READ_WRITE_TOKEN, WindowSize,
};

const WAIT: Duration = Duration::from_secs(5);

/// The embedder's half: what a pseudo-console would expose.
struct Loopback {
    io: TcpStream,
    child_signal: TcpStream,
    exit_events: mpsc::Receiver<ChildEvent>,
    sizes: Arc<Mutex<Vec<WindowSize>>>,
}

/// The "child" half: writes output, reads input, reports exit.
struct Remote {
    io: TcpStream,
    child_signal: TcpStream,
    exit_events: mpsc::Sender<ChildEvent>,
    sizes: Arc<Mutex<Vec<WindowSize>>>,
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
    (
        Loopback {
            io,
            child_signal,
            exit_events,
            sizes: Arc::clone(&sizes),
        },
        Remote {
            io: remote_io,
            child_signal: remote_signal,
            exit_events: events_tx,
            sizes,
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
            poller.add_with_mode(
                &self.child_signal,
                Event::readable(PTY_CHILD_EVENT_TOKEN),
                PollMode::Level,
            )
        }
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

impl OnResize for Loopback {
    fn on_resize(&mut self, size: WindowSize) -> io::Result<()> {
        self.sizes.lock().unwrap().push(size);
        Ok(())
    }
}

/// Waits for the next event and returns its key.
fn next_key(poller: &Poller) -> usize {
    let mut events = Events::new();
    assert!(
        poller.wait(&mut events, Some(WAIT)).unwrap() > 0,
        "no event in time"
    );
    events.iter().next().unwrap().key
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
