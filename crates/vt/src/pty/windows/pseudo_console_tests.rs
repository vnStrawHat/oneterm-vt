//! Real `cmd.exe` behind a real pseudo-console, read without a `Poller`.

use std::io::{self, Read, Write};
use std::thread;
use std::time::{Duration, Instant};

use super::*;
use crate::pty::Shell;

const WAIT: Duration = Duration::from_secs(15);

fn cmd(args: &[&str]) -> PseudoConsole {
    let options = Options {
        shell: Some(Shell::new(
            "cmd.exe".to_owned(),
            args.iter().map(|arg| (*arg).to_owned()).collect(),
        )),
        ..Options::default()
    };
    let size = WindowSize {
        rows: 24,
        cols: 80,
        cell_width: 8,
        cell_height: 16,
    };
    PseudoConsole::spawn(&options, size).unwrap()
}

/// Reads until `done(output)` or `WAIT` passes.
fn read_until(pty: &mut PseudoConsole, done: impl Fn(&str) -> bool) -> String {
    let deadline = Instant::now() + WAIT;
    let mut output = Vec::new();
    let mut buf = [0u8; 4096];
    while Instant::now() < deadline {
        match pty.reader().read(&mut buf) {
            Ok(0) => break,
            Ok(n) => output.extend_from_slice(&buf[..n]),
            Err(cause) if cause.kind() == io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(10));
            }
            Err(cause) => panic!("read failed: {cause}"),
        }
        if done(&String::from_utf8_lossy(&output)) {
            break;
        }
    }
    String::from_utf8_lossy(&output).into_owned()
}

#[test]
fn output_arrives() {
    let mut pty = cmd(&["/d", "/c", "echo vt-pty-ok"]);
    let output = read_until(&mut pty, |text| text.contains("vt-pty-ok"));
    assert!(output.contains("vt-pty-ok"), "{output:?}");
}

#[test]
fn typed_input_echoes() {
    let mut pty = cmd(&["/d", "/q", "/k", "echo off"]);
    // `^e` is an escaped `e`: the typed line shows `typ^ed`, only the command's output is
    // `typed`, so the marker proves the shell ran what was written.
    pty.writer().write_all(b"echo vt-pty-typ^ed\r\n").unwrap();
    let output = read_until(&mut pty, |text| text.contains("vt-pty-typed"));
    assert!(output.contains("vt-pty-typed"), "{output:?}");
}

#[test]
fn exit_code_reported() {
    let mut pty = cmd(&["/d", "/c", "exit 3"]);
    let deadline = Instant::now() + WAIT;
    let event = loop {
        if let Some(event) = pty.next_child_event() {
            break event;
        }
        assert!(Instant::now() < deadline, "the child did not exit in time");
        thread::sleep(Duration::from_millis(10));
    };
    let ChildEvent::Exited(status) = event;
    assert_eq!(status.and_then(|status| status.code()), Some(3));
    assert_eq!(pty.next_child_event(), None);
}

#[test]
fn child_pid_is_positive() {
    let pty = cmd(&["/d", "/c", "exit 0"]);
    assert!(pty.child_pid().is_some_and(|pid| pid > 0));
}

#[test]
fn resize_of_a_live_console_succeeds() {
    let mut pty = cmd(&["/d", "/q", "/k", "echo off"]);
    let size = WindowSize {
        rows: 30,
        cols: 100,
        ..WindowSize::default()
    };
    pty.on_resize(size).unwrap();
    let err = pty
        .on_resize(WindowSize {
            rows: 40000,
            ..size
        })
        .unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
}

#[test]
fn missing_program_names_it() {
    let options = Options {
        shell: Some(Shell::new(
            "vt-pty-no-such-program.exe".to_owned(),
            Vec::new(),
        )),
        ..Options::default()
    };
    let started = Instant::now();
    let cause = PseudoConsole::spawn(
        &options,
        WindowSize {
            rows: 24,
            cols: 80,
            ..WindowSize::default()
        },
    )
    .err()
    .unwrap();
    assert!(
        cause
            .to_string()
            .starts_with("cannot start 'vt-pty-no-such-program.exe': "),
        "{cause}"
    );
    // The failed spawn must close the console without waiting for output nobody reads.
    assert!(started.elapsed() < Duration::from_secs(20));
}

#[test]
fn drop_with_unread_output_finishes() {
    // Guards hosts before build 26100, whose `ClosePseudoConsole` waits for the output to be
    // drained: the dropped reader must discard instead.
    let mut pty = cmd(&["/d", "/c", "dir /s C:\\Windows\\System32\\drivers"]);
    read_until(&mut pty, |text| text.len() > 1024);
    let dropper = thread::spawn(move || drop(pty));
    let deadline = Instant::now() + Duration::from_secs(20);
    while !dropper.is_finished() {
        assert!(Instant::now() < deadline, "drop did not finish");
        thread::sleep(Duration::from_millis(20));
    }
    dropper.join().unwrap();
}

#[test]
fn oversized_window_fails_spawn() {
    let size = WindowSize {
        rows: 40000,
        cols: 80,
        ..WindowSize::default()
    };
    let cause = PseudoConsole::spawn(&Options::default(), size)
        .err()
        .unwrap();
    assert_eq!(cause.kind(), io::ErrorKind::InvalidInput);
    assert_eq!(cause.to_string(), "window size 80x40000 exceeds 32767");
}
