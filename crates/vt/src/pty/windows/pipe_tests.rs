//! Ring behaviour over `std::io::pipe()`, with a 4096-byte ring.

use std::io::{self, Read, Write};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use polling::{Event, Events, PollMode, Poller};

use super::*;
use crate::pty::PTY_READ_WRITE_TOKEN;

const RING: usize = 4096;
const WAIT: Duration = Duration::from_secs(5);

fn poller() -> Arc<Poller> {
    Arc::new(Poller::new().unwrap())
}

fn reader_over_pipe() -> (PipeReader, io::PipeWriter) {
    let (rx, tx) = io::pipe().unwrap();
    (PipeReader::new(OwnedHandle::from(rx), RING).unwrap(), tx)
}

fn writer_over_pipe() -> (PipeWriter, io::PipeReader) {
    let (rx, tx) = io::pipe().unwrap();
    (PipeWriter::new(OwnedHandle::from(tx), RING).unwrap(), rx)
}

/// Waits up to `first` for a packet, then counts packets until `quiet` passes without one.
fn packets(poller: &Poller, first: Duration, quiet: Duration) -> usize {
    let mut events = Events::new();
    let mut count = 0;
    let mut timeout = first;
    loop {
        events.clear();
        if poller.wait(&mut events, Some(timeout)).unwrap() == 0 {
            return count;
        }
        timeout = quiet;
        count += events
            .iter()
            .filter(|event| event.key == PTY_READ_WRITE_TOKEN)
            .count();
    }
}

fn drain_packets(poller: &Poller) {
    packets(
        poller,
        Duration::from_millis(50),
        Duration::from_millis(100),
    );
}

fn wait_until(what: &str, mut done: impl FnMut() -> bool) {
    let deadline = Instant::now() + WAIT;
    while !done() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        thread::sleep(Duration::from_millis(5));
    }
}

fn buffered(reader: &PipeReader) -> usize {
    reader.ring.lock().bytes.len()
}

#[test]
fn reader_with_bytes_left_is_woken_again() {
    let poller = poller();
    let (mut reader, mut tx) = reader_over_pipe();
    reader.set_interest(
        &poller,
        Event::readable(PTY_READ_WRITE_TOKEN),
        PollMode::Level,
    );
    tx.write_all(b"0123456789").unwrap();
    wait_until("the bytes", || buffered(&reader) == 10);
    drain_packets(&poller);

    let mut part = [0u8; 4];
    assert_eq!(reader.read(&mut part).unwrap(), 4);
    assert_eq!(&part, b"0123");
    // Six bytes are left: level semantics wake the embedder again.
    assert_eq!(packets(&poller, WAIT, Duration::from_millis(50)), 1);
    let mut rest = [0u8; 16];
    assert_eq!(reader.read(&mut rest).unwrap(), 6);
}

#[test]
fn wakes_posted_before_a_poll_all_arrive() {
    let poller = poller();
    let (reader, mut tx) = reader_over_pipe();
    reader.set_interest(
        &poller,
        Event::readable(PTY_READ_WRITE_TOKEN),
        PollMode::Level,
    );
    tx.write_all(b"a").unwrap();
    wait_until("the first byte", || buffered(&reader) == 1);
    tx.write_all(b"b").unwrap();
    wait_until("the second byte", || buffered(&reader) == 2);
    // The priming packet and one per push, none of them lost while nobody polled.
    assert_eq!(packets(&poller, WAIT, Duration::from_millis(100)), 3);
}

#[test]
fn oneshot_interest_is_spent_by_a_wake() {
    let poller = poller();
    let (mut reader, mut tx) = reader_over_pipe();
    reader.set_interest(
        &poller,
        Event::readable(PTY_READ_WRITE_TOKEN),
        PollMode::Oneshot,
    );
    assert_eq!(packets(&poller, WAIT, Duration::from_millis(100)), 1);
    tx.write_all(b"x").unwrap();
    wait_until("the byte", || buffered(&reader) == 1);
    assert_eq!(
        packets(
            &poller,
            Duration::from_millis(200),
            Duration::from_millis(100)
        ),
        0
    );
    // Re-arming with bytes pending wakes at once.
    reader.set_interest(
        &poller,
        Event::readable(PTY_READ_WRITE_TOKEN),
        PollMode::Oneshot,
    );
    assert_eq!(packets(&poller, WAIT, Duration::from_millis(100)), 1);
    let mut byte = [0u8; 1];
    assert_eq!(reader.read(&mut byte).unwrap(), 1);
}

#[test]
fn writer_with_room_is_woken_again() {
    let poller = poller();
    let (mut writer, _rx) = writer_over_pipe();
    writer.set_interest(
        &poller,
        Event::writable(PTY_READ_WRITE_TOKEN),
        PollMode::Level,
    );
    drain_packets(&poller);
    assert_eq!(writer.write(b"abc").unwrap(), 3);
    assert!(packets(&poller, WAIT, Duration::from_millis(50)) >= 1);
}

#[test]
fn empty_reader_would_block() {
    let (mut reader, _tx) = reader_over_pipe();
    let error = reader.read(&mut [0u8; 8]).unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::WouldBlock);
}

#[test]
fn reader_sees_eof_after_pipe_closes() {
    let poller = poller();
    let (mut reader, tx) = reader_over_pipe();
    reader.set_interest(
        &poller,
        Event::readable(PTY_READ_WRITE_TOKEN),
        PollMode::Level,
    );
    drain_packets(&poller);
    drop(tx);
    assert_eq!(packets(&poller, WAIT, Duration::from_millis(50)), 1);
    assert_eq!(reader.read(&mut [0u8; 8]).unwrap(), 0);
}

#[test]
fn bytes_before_eof_are_delivered_first() {
    let (mut reader, mut tx) = reader_over_pipe();
    tx.write_all(b"tail").unwrap();
    drop(tx);
    wait_until("the end of output", || reader.ring.lock().ended);
    let mut buf = [0u8; 16];
    assert_eq!(reader.read(&mut buf).unwrap(), 4);
    assert_eq!(reader.read(&mut buf).unwrap(), 0);
}

#[test]
fn full_writer_would_block() {
    let (mut writer, _rx) = writer_over_pipe();
    let block = [7u8; 1024];
    let mut accepted = 0usize;
    let error = loop {
        match writer.write(&block) {
            Ok(0) => panic!("a write must not return Ok(0) for a non-empty buffer"),
            Ok(n) => accepted += n,
            Err(error) => break error,
        }
        assert!(accepted < 64 * 1024, "the ring never filled up");
    };
    assert_eq!(error.kind(), io::ErrorKind::WouldBlock);
}

#[test]
fn written_bytes_reach_the_pipe() {
    let (mut writer, mut rx) = writer_over_pipe();
    writer.write_all(b"hello").unwrap();
    let mut buf = [0u8; 5];
    rx.read_exact(&mut buf).unwrap();
    assert_eq!(&buf, b"hello");
}

#[test]
fn dropped_reader_keeps_draining() {
    let (reader, mut tx) = reader_over_pipe();
    let producer = thread::spawn(move || {
        // Far more than the ring and the OS pipe hold together.
        tx.write_all(&vec![1u8; RING * 16]).unwrap();
    });
    thread::sleep(Duration::from_millis(200));
    assert!(!producer.is_finished(), "nothing was reading yet");
    drop(reader);
    wait_until("the writes to finish", || producer.is_finished());
    producer.join().unwrap();
}

#[test]
fn input_thread_survives_the_ring_closing_during_a_blocked_write() {
    let (mut rx, tx) = io::pipe().unwrap();
    let ring = Ring::new(Direction::Input, RING);
    let thread_ring = Arc::clone(&ring);
    let pipe = OwnedHandle::from(tx);
    let pump = thread::spawn(move || pump_input(&thread_ring, &pipe));
    // Far more than the OS pipe buffer holds, put in directly: the writer would stop at RING.
    {
        let mut state = ring.lock();
        state.bytes.extend(std::iter::repeat_n(7u8, 1 << 20));
        ring.changed.notify_all();
    }
    // Nobody reads the pipe, so the thread ends up blocked inside `WriteFile`: the ring stops
    // shrinking.
    let mut left = ring.lock().bytes.len();
    wait_until("the thread to block in WriteFile", || {
        thread::sleep(Duration::from_millis(100));
        let now = ring.lock().bytes.len();
        let stuck = now == left;
        left = now;
        stuck
    });
    // The owner goes away while the write is still blocked.
    ring.close();
    // Reading the pipe lets the write finish; it must see the closed ring and return.
    let mut buf = [0u8; 8192];
    while rx.read(&mut buf).unwrap() > 0 {}
    assert!(pump.join().is_ok(), "the input thread panicked");
}
