//! Anonymous pipes made pollable.
//!
//! Anonymous pipes cannot join the I/O completion port `polling` uses, so each direction has
//! one thread doing blocking `ReadFile` / `WriteFile` and a bounded ring between that thread
//! and the embedder. The ring posts completion packets to the registered poller. The embedder
//! sees POSIX semantics on a non-blocking descriptor: `WouldBlock` when there is nothing to
//! read or no room to write, `Ok(0)` only at end of output.

#![expect(
    unsafe_code,
    reason = "Win32 FFI: ReadFile and WriteFile on owned pipe handles"
)]

use std::collections::VecDeque;
use std::io;
use std::os::windows::io::{AsRawHandle, OwnedHandle};
use std::ptr;
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::thread;

use polling::os::iocp::{CompletionPacket, PollerIocpExt};
use polling::{Event, PollMode, Poller};
use windows_sys::Win32::Storage::FileSystem::{ReadFile, WriteFile};

/// Largest single `ReadFile` / `WriteFile`.
const CHUNK: usize = 64 * 1024;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Direction {
    /// Console output: the thread reads the pipe, the embedder reads the ring.
    Output,
    /// Console input: the embedder writes the ring, the thread writes the pipe.
    Input,
}

struct Interest {
    /// Built once per registration and cloned per post.
    packet: CompletionPacket,
    poller: Arc<Poller>,
    mode: PollMode,
    readable: bool,
    writable: bool,
}

struct State {
    bytes: VecDeque<u8>,
    /// Output: the pipe ended. Input: the pipe broke.
    ended: bool,
    /// The owning `PipeReader` / `PipeWriter` was dropped.
    closed: bool,
    /// The first registration posts once even when nothing is ready.
    primed: bool,
    interest: Option<Interest>,
}

impl State {
    /// Posts one packet when the registered interest covers `direction`. `Oneshot` modes drop
    /// the interest, as `polling` does for a real source.
    ///
    // ponytail: one packet per push, never coalesced. An embedder that stops polling lets the
    // IOCP queue grow by a packet per chunk (bounded by the pipe's throughput). Upgrade: a
    // `wake_pending` flag in `State`, set on post and cleared on read/write.
    fn wake(&mut self, direction: Direction) {
        let Some(interest) = &self.interest else {
            return;
        };
        let wanted = match direction {
            Direction::Output => interest.readable,
            Direction::Input => interest.writable,
        };
        if !wanted {
            return;
        }
        if let Err(cause) = interest.poller.post(interest.packet.clone()) {
            log::warn!("pty: cannot post a completion packet: {cause}");
        }
        if matches!(interest.mode, PollMode::Oneshot | PollMode::EdgeOneshot) {
            self.interest = None;
        }
    }

    /// Level emulation: a read that leaves bytes, or a write that leaves room, wakes again.
    fn wake_again_if_level(&mut self, direction: Direction, still_ready: bool) {
        let is_level = self
            .interest
            .as_ref()
            .is_some_and(|interest| interest.mode == PollMode::Level);
        if is_level && still_ready {
            self.wake(direction);
        }
    }
}

/// The bounded buffer between a pipe thread and the embedder.
struct Ring {
    direction: Direction,
    capacity: usize,
    state: Mutex<State>,
    /// Signalled when bytes move or the owner closes: wakes the pipe thread.
    changed: Condvar,
}

impl Ring {
    fn new(direction: Direction, capacity: usize) -> Arc<Self> {
        Arc::new(Self {
            direction,
            capacity,
            state: Mutex::new(State {
                bytes: VecDeque::new(),
                ended: false,
                closed: false,
                primed: false,
                interest: None,
            }),
            changed: Condvar::new(),
        })
    }

    /// Bytes survive a panic elsewhere, so poisoning is ignored.
    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn wait<'a>(&self, state: MutexGuard<'a, State>) -> MutexGuard<'a, State> {
        self.changed
            .wait(state)
            .unwrap_or_else(PoisonError::into_inner)
    }

    fn chunk_len(&self) -> usize {
        CHUNK.min(self.capacity)
    }

    fn set_interest(&self, poller: &Arc<Poller>, event: Event, mode: PollMode) {
        let side = match self.direction {
            Direction::Output => Event::readable(event.key),
            Direction::Input => Event::writable(event.key),
        };
        let mut state = self.lock();
        state.interest = Some(Interest {
            packet: CompletionPacket::new(side),
            poller: Arc::clone(poller),
            mode,
            readable: event.readable,
            writable: event.writable,
        });
        let ready = match self.direction {
            Direction::Output => !state.primed || !state.bytes.is_empty() || state.ended,
            Direction::Input => state.bytes.len() < self.capacity && !state.ended,
        };
        state.primed = true;
        if ready {
            state.wake(self.direction);
        }
    }

    fn clear_interest(&self) {
        self.lock().interest = None;
    }

    /// The owner is gone: stop waking, free the buffer, release the pipe thread.
    fn close(&self) {
        let mut state = self.lock();
        state.closed = true;
        state.interest = None;
        state.bytes.clear();
        self.changed.notify_all();
    }
}

fn spawn_pipe_thread(name: &str, body: impl FnOnce() + Send + 'static) -> io::Result<()> {
    thread::Builder::new()
        .name(name.to_owned())
        .spawn(body)
        .map(|_| ())
        .map_err(|cause| io::Error::new(cause.kind(), format!("{name} thread failed: {cause}")))
}

/// Console output as an `io::Read`: empty means `WouldBlock`, `Ok(0)` only at end of output.
pub struct PipeReader {
    ring: Arc<Ring>,
}

impl PipeReader {
    pub(super) fn new(pipe: OwnedHandle, capacity: usize) -> io::Result<Self> {
        let ring = Ring::new(Direction::Output, capacity);
        let thread_ring = Arc::clone(&ring);
        spawn_pipe_thread("oneterm-vt-pty-conout", move || {
            pump_output(&thread_ring, &pipe);
        })?;
        Ok(Self { ring })
    }

    pub(super) fn set_interest(&self, poller: &Arc<Poller>, event: Event, mode: PollMode) {
        self.ring.set_interest(poller, event, mode);
    }

    pub(super) fn clear_interest(&self) {
        self.ring.clear_interest();
    }
}

impl io::Read for PipeReader {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        let mut state = self.ring.lock();
        if state.bytes.is_empty() {
            return if state.ended {
                Ok(0)
            } else {
                Err(io::ErrorKind::WouldBlock.into())
            };
        }
        let (front, back) = state.bytes.as_slices();
        let from_front = front.len().min(buf.len());
        buf[..from_front].copy_from_slice(&front[..from_front]);
        let from_back = back.len().min(buf.len() - from_front);
        buf[from_front..from_front + from_back].copy_from_slice(&back[..from_back]);
        let copied = from_front + from_back;
        state.bytes.drain(..copied);
        self.ring.changed.notify_all();
        let more = !state.bytes.is_empty();
        state.wake_again_if_level(Direction::Output, more);
        Ok(copied)
    }
}

impl Drop for PipeReader {
    fn drop(&mut self) {
        // The thread keeps reading and discarding until the pipe breaks, so a console that
        // waits for its output to be drained can finish closing.
        self.ring.close();
    }
}

/// Console input as an `io::Write`: a full buffer means `WouldBlock`; writes may be short.
pub struct PipeWriter {
    ring: Arc<Ring>,
}

impl PipeWriter {
    pub(super) fn new(pipe: OwnedHandle, capacity: usize) -> io::Result<Self> {
        let ring = Ring::new(Direction::Input, capacity);
        let thread_ring = Arc::clone(&ring);
        spawn_pipe_thread("oneterm-vt-pty-conin", move || {
            pump_input(&thread_ring, &pipe);
        })?;
        Ok(Self { ring })
    }

    pub(super) fn set_interest(&self, poller: &Arc<Poller>, event: Event, mode: PollMode) {
        self.ring.set_interest(poller, event, mode);
    }

    pub(super) fn clear_interest(&self) {
        self.ring.clear_interest();
    }
}

impl io::Write for PipeWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        let mut state = self.ring.lock();
        if state.ended {
            return Err(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "the pseudo-console input pipe is closed",
            ));
        }
        let room = self.ring.capacity - state.bytes.len();
        if room == 0 {
            return Err(io::ErrorKind::WouldBlock.into());
        }
        let accepted = room.min(buf.len());
        state.bytes.extend(&buf[..accepted]);
        self.ring.changed.notify_all();
        let has_room = state.bytes.len() < self.ring.capacity;
        state.wake_again_if_level(Direction::Input, has_room);
        Ok(accepted)
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl Drop for PipeWriter {
    fn drop(&mut self) {
        self.ring.close();
    }
}

/// Reads the pipe into the ring until the pipe ends; after the owner closes, keeps reading and
/// discarding until the pipe breaks.
fn pump_output(ring: &Ring, pipe: &OwnedHandle) {
    let mut chunk = vec![0u8; ring.chunk_len()];
    while let Some(read) = read_pipe(pipe, &mut chunk) {
        let mut state = ring.lock();
        loop {
            if state.closed {
                break;
            }
            if ring.capacity - state.bytes.len() >= read {
                state.bytes.extend(&chunk[..read]);
                state.wake(Direction::Output);
                break;
            }
            state = ring.wait(state);
        }
    }
    let mut state = ring.lock();
    state.ended = true;
    state.wake(Direction::Output);
}

/// Writes the ring to the pipe until the owner closes or the pipe breaks.
fn pump_input(ring: &Ring, pipe: &OwnedHandle) {
    let mut chunk = vec![0u8; ring.chunk_len()];
    loop {
        let len = {
            let mut state = ring.lock();
            while state.bytes.is_empty() && !state.closed {
                state = ring.wait(state);
            }
            if state.closed {
                return;
            }
            let front = state.bytes.make_contiguous();
            let len = front.len().min(chunk.len());
            chunk[..len].copy_from_slice(&front[..len]);
            len
        };
        // The bytes stay in the ring while they are written, so the ring bounds all
        // buffered input.
        let written = write_pipe(pipe, &chunk[..len]);
        let mut state = ring.lock();
        match written {
            Some(written) => {
                // `close` cleared the ring during the blocking write: nothing left to drain.
                if state.closed {
                    return;
                }
                state.bytes.drain(..written);
                state.wake(Direction::Input);
            }
            None => {
                state.ended = true;
                state.bytes.clear();
                state.wake(Direction::Input);
                return;
            }
        }
    }
}

/// One blocking read; `None` when the pipe ended or broke.
fn read_pipe(pipe: &OwnedHandle, buf: &mut [u8]) -> Option<usize> {
    let mut read = 0u32;
    let len = u32::try_from(buf.len()).unwrap_or(u32::MAX);
    // SAFETY: `buf` is valid for writes of `len` bytes for the whole call (the length is
    // clamped to `u32`), `read` is a live out-parameter, there is no OVERLAPPED, and the handle
    // is owned by the calling thread's closure.
    let ok = unsafe {
        ReadFile(
            pipe.as_raw_handle(),
            buf.as_mut_ptr(),
            len,
            &mut read,
            ptr::null_mut(),
        )
    };
    (ok != 0 && read > 0).then_some(read as usize)
}

/// One blocking write; `None` when the pipe broke. Returns how many bytes were written.
fn write_pipe(pipe: &OwnedHandle, buf: &[u8]) -> Option<usize> {
    let mut written = 0u32;
    let len = u32::try_from(buf.len()).unwrap_or(u32::MAX);
    // SAFETY: `buf` is valid for reads of `len` bytes for the whole call (the length is
    // clamped to `u32`), `written` is a live out-parameter, there is no OVERLAPPED, and the
    // handle is owned by the calling thread's closure.
    let ok = unsafe {
        WriteFile(
            pipe.as_raw_handle(),
            buf.as_ptr(),
            len,
            &mut written,
            ptr::null_mut(),
        )
    };
    (ok != 0 && written > 0).then_some(written as usize)
}

#[cfg(test)]
#[path = "pipe_tests.rs"]
mod pipe_tests;
