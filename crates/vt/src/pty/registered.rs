//! A console that stays registered on a poller for as long as it lives.

use std::io::{self, Read, Write};
use std::sync::Arc;

use polling::{Event, PollMode, Poller};

use super::{ChildEvent, EventedPty, EventedReadWrite, OnResize, PseudoConsole, WindowSize};

/// A console registered on a poller, with nothing `unsafe` left to the caller.
///
/// It owns the console and holds the poller, and it deregisters before the console is dropped:
/// the rule [`EventedReadWrite::register`] asks its callers to keep. Other clones of the
/// poller's `Arc` may outlive it.
///
/// Output is read and input is written through [`io::Read`] and [`io::Write`] on the
/// registration itself, with the return values of the [module](super)'s "Reading and writing"
/// table. The console's reader and writer are not handed out: swapped between two
/// registrations in safe code, each would be closed while the other's poller still holds it.
pub struct Registered<C: EventedReadWrite = PseudoConsole> {
    // Dropped in this order, after `drop` deregistered: the console, then the poller.
    console: C,
    poller: Arc<Poller>,
}

impl<C: EventedReadWrite> Registered<C> {
    /// Registers `console` on `poller` as [`EventedReadWrite::register`] does: I/O under
    /// [`PTY_READ_WRITE_TOKEN`](super::PTY_READ_WRITE_TOKEN) with `interest`'s readable and
    /// writable flags, child events under
    /// [`PTY_CHILD_EVENT_TOKEN`](super::PTY_CHILD_EVENT_TOKEN).
    ///
    /// On failure nothing stays registered and the console is dropped.
    #[expect(
        unsafe_code,
        reason = "calls the unsafe register and keeps its rule in Drop"
    )]
    pub fn new(
        mut console: C,
        poller: Arc<Poller>,
        interest: Event,
        mode: PollMode,
    ) -> io::Result<Self> {
        // SAFETY: from here on `Self` owns the console and a clone of the poller's `Arc`,
        // never hands out a `&mut` to the console or to its sources, and deregisters in `drop`
        // before either is dropped. A failed registration is undone right below, before the
        // console is dropped.
        let registered = unsafe { console.register(&poller, interest, mode) };
        if let Err(cause) = registered {
            // A failed call may have added some of the console's sources.
            let _ = console.deregister(&poller);
            return Err(cause);
        }
        Ok(Self { console, poller })
    }

    /// The poller: wait on it, and wake it from other threads with `Poller::notify`.
    pub fn poller(&self) -> &Arc<Poller> {
        &self.poller
    }

    /// The console, for what it answers without being changed (`PseudoConsole::child_pid`).
    pub fn console(&self) -> &C {
        &self.console
    }

    /// Changes the interest in reads and writes; the child side is unchanged.
    pub fn reregister(&mut self, interest: Event, mode: PollMode) -> io::Result<()> {
        self.console.reregister(&self.poller, interest, mode)
    }
}

impl<C: EventedPty> Registered<C> {
    /// The next child event, as [`EventedPty::next_child_event`] gives it.
    pub fn next_child_event(&mut self) -> Option<ChildEvent> {
        self.console.next_child_event()
    }
}

impl<C: EventedReadWrite + OnResize> Registered<C> {
    /// Tells the child the new size, as [`OnResize::on_resize`] does.
    pub fn on_resize(&mut self, size: WindowSize) -> io::Result<()> {
        self.console.on_resize(size)
    }
}

impl<C: EventedReadWrite> Read for Registered<C> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.console.reader().read(buf)
    }
}

impl<C: EventedReadWrite> Write for Registered<C> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.console.writer().write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.console.writer().flush()
    }
}

impl<C: EventedReadWrite> Drop for Registered<C> {
    fn drop(&mut self) {
        if let Err(cause) = self.console.deregister(&self.poller) {
            log::warn!("pty: cannot deregister the console: {cause}");
        }
    }
}
