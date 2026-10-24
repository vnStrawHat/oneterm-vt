//! Windows half of the pseudo-console: ConPTY, two pipe threads and a process wait.

mod child;
mod conpty;
mod pipe;
#[cfg(test)]
mod pseudo_console_tests;

use std::ffi::OsStr;
use std::io;
use std::os::windows::ffi::OsStrExt;
use std::sync::Arc;

use polling::{Event, PollMode, Poller};
use windows_sys::Win32::System::Console::COORD;

use self::child::ChildExitWatcher;
use self::conpty::{Conpty, SpawnRequest, environment_block};
pub use self::pipe::{PipeReader, PipeWriter};
use super::{
    ChildEvent, EventedPty, EventedReadWrite, OnResize, Options, PTY_CHILD_EVENT_TOKEN,
    PTY_READ_WRITE_TOKEN, WindowSize, validate_env,
};

/// Capacity of each pipe ring.
const PIPE_CAPACITY: usize = 1 << 20;

/// What `Options::shell: None` starts.
const DEFAULT_PROGRAM: &str = "powershell";

/// A client behind a ConPTY pseudo-console, polled through two pipe threads and a process wait.
///
/// The field order is the drop order: the reader switches to drain-and-discard first, so the
/// console can close while the host still has output to flush; then the writer; then the
/// watcher gives the child a grace period and terminates it.
pub struct PseudoConsole {
    conout: PipeReader,
    conpty: Conpty,
    conin: PipeWriter,
    child: ChildExitWatcher,
}

impl PseudoConsole {
    /// Starts `options.shell` (default: `powershell`) behind a new pseudo-console of `size`.
    /// Uses `conpty.dll` beside the executable when present, else the console host of the
    /// running Windows.
    pub fn spawn(options: &Options, size: WindowSize) -> io::Result<Self> {
        validate_env(&options.env)?;
        let size = coord(size)?;
        let (program, args) = match &options.shell {
            Some(shell) => (shell.program(), shell.args()),
            None => (DEFAULT_PROGRAM, &[][..]),
        };
        let command_line = cmdline(program, args, options.escape_args);
        let request = SpawnRequest {
            program: if options.shell.is_some() {
                program
            } else {
                "the default shell"
            },
            command_line: win32_string(OsStr::new(&command_line)),
            working_directory: options
                .working_directory
                .as_deref()
                .map(|directory| win32_string(directory.as_os_str())),
            environment: environment_block(&options.env, std::env::vars_os()),
            size,
            glyph_width: options.glyph_width,
        };
        let spawned = conpty::spawn(request)?;
        // Built as locals in the reverse of the drop order, so a `?` drops what exists in the
        // field order below (a `?` inside a struct literal would not). The watcher comes first:
        // once the child runs, its drop gives the child a grace period and terminates it.
        let child = ChildExitWatcher::new(spawned.process)?;
        let conin = PipeWriter::new(spawned.conin, PIPE_CAPACITY)?;
        let conpty = spawned.conpty;
        let conout = PipeReader::new(spawned.conout, PIPE_CAPACITY)?;
        Ok(Self {
            conout,
            conpty,
            conin,
            child,
        })
    }

    /// The child's process id; `None` if Windows reported 0.
    pub fn child_pid(&self) -> Option<u32> {
        self.child.pid()
    }
}

impl EventedReadWrite for PseudoConsole {
    type Reader = PipeReader;
    type Writer = PipeWriter;

    #[expect(unsafe_code, reason = "implements the unsafe register")]
    unsafe fn register(
        &mut self,
        poller: &Arc<Poller>,
        interest: Event,
        mode: PollMode,
    ) -> io::Result<()> {
        // No OS source is added: the pipe rings and the exit watcher post packets instead.
        self.reregister(poller, interest, mode)
    }

    fn reregister(
        &mut self,
        poller: &Arc<Poller>,
        interest: Event,
        mode: PollMode,
    ) -> io::Result<()> {
        let mut interest = interest;
        interest.key = PTY_READ_WRITE_TOKEN;
        self.conout.set_interest(poller, interest, mode);
        self.conin.set_interest(poller, interest, mode);
        self.child
            .register(poller, Event::readable(PTY_CHILD_EVENT_TOKEN));
        Ok(())
    }

    fn deregister(&mut self, _poller: &Arc<Poller>) -> io::Result<()> {
        self.conout.clear_interest();
        self.conin.clear_interest();
        self.child.deregister();
        Ok(())
    }

    fn reader(&mut self) -> &mut PipeReader {
        &mut self.conout
    }

    fn writer(&mut self) -> &mut PipeWriter {
        &mut self.conin
    }
}

impl EventedPty for PseudoConsole {
    fn next_child_event(&mut self) -> Option<ChildEvent> {
        self.child.next_event()
    }
}

impl OnResize for PseudoConsole {
    fn on_resize(&mut self, size: WindowSize) -> io::Result<()> {
        // The pixel fields have no ConPTY equivalent.
        self.conpty.resize(coord(size)?)
    }
}

/// `COORD` fields are `SHORT`: larger values would wrap negative.
fn coord(size: WindowSize) -> io::Result<COORD> {
    let too_large = || {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "window size {}x{} exceeds {}",
                size.cols,
                size.rows,
                i16::MAX
            ),
        )
    };
    Ok(COORD {
        X: i16::try_from(size.cols).map_err(|_| too_large())?,
        Y: i16::try_from(size.rows).map_err(|_| too_large())?,
    })
}

/// UTF-16, NUL-terminated.
fn win32_string(text: &OsStr) -> Vec<u16> {
    text.encode_wide().chain(Some(0)).collect()
}

/// The program, then each argument after a space: verbatim, or escaped with the C-runtime
/// rules when `escape_args` is set (which also quotes a program name holding a space or tab,
/// so `C:\Program Files\x.exe` cannot run `C:\Program.exe`).
fn cmdline(program: &str, args: &[String], escape_args: bool) -> String {
    let mut line = String::new();
    let needs_quotes = escape_args && program.contains([' ', '\t']) && !program.contains('"');
    if needs_quotes {
        line.push('"');
        line.push_str(program);
        line.push('"');
    } else {
        line.push_str(program);
    }
    for arg in args {
        line.push(' ');
        if escape_args {
            push_escaped_arg(&mut line, arg);
        } else {
            line.push_str(arg);
        }
    }
    line
}

/// Appends `arg` so that the C runtime parses it back to `arg`: quoted when empty or holding a
/// space or tab; a run of backslashes before a quote is doubled and the quote escaped; a
/// trailing run is doubled when the closing quote follows.
fn push_escaped_arg(line: &mut String, arg: &str) {
    let quoted = arg.is_empty() || arg.contains([' ', '\t']);
    if quoted {
        line.push('"');
    }
    // Length of the run of backslashes that ends just before the current character.
    let mut run = 0usize;
    for ch in arg.chars() {
        match ch {
            '\\' => run += 1,
            '"' => {
                // Extend the run to 2n+1 backslashes: it escapes the quote itself.
                line.extend(std::iter::repeat_n('\\', run + 1));
                run = 0;
            }
            _ => run = 0,
        }
        line.push(ch);
    }
    if quoted {
        // The closing quote must not be escaped by a trailing run: double it.
        line.extend(std::iter::repeat_n('\\', run));
        line.push('"');
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pty::Shell;

    fn escaped(arg: &str) -> String {
        let mut line = String::new();
        push_escaped_arg(&mut line, arg);
        line
    }

    #[test]
    fn escaping_table() {
        let table = [
            ("abc", "abc"),
            ("", "\"\""),
            (" ", "\" \""),
            ("ab c", "\"ab c\""),
            ("ab\tc", "\"ab\tc\""),
            ("ab\\c", "ab\\c"),
            ("ab\"c", "ab\\\"c"),
            ("\"", "\\\""),
            ("a\"b\"c", "a\\\"b\\\"c"),
            ("ab \"c", "\"ab \\\"c\""),
            ("a \"b\" c", "\"a \\\"b\\\" c\""),
            ("C:\\Program Files\\", "\"C:\\Program Files\\\\\""),
            ("C:\\Program Files\\a.txt", "\"C:\\Program Files\\a.txt\""),
        ];
        for (input, expected) in table {
            assert_eq!(escaped(input), expected, "input {input:?}");
        }
    }

    #[test]
    fn escapes_only_when_asked() {
        let args = ["/c".to_owned(), "echo hello world".to_owned()];
        assert_eq!(
            cmdline("cmd.exe", &args, false),
            "cmd.exe /c echo hello world"
        );
        assert_eq!(
            cmdline("cmd.exe", &args, true),
            "cmd.exe /c \"echo hello world\""
        );
    }

    #[test]
    fn default_shell_is_powershell() {
        let options = Options::default();
        assert!(options.shell.is_none());
        assert_eq!(DEFAULT_PROGRAM, "powershell");
        assert_eq!(Shell::default().program(), "");
    }

    #[test]
    fn program_quoted_only_with_escape_args() {
        let program = "C:\\Program Files\\x.exe";
        let args = ["a".to_owned()];
        assert_eq!(
            cmdline(program, &args, true),
            "\"C:\\Program Files\\x.exe\" a"
        );
        assert_eq!(cmdline(program, &args, false), "C:\\Program Files\\x.exe a");
        // A program that already holds a quote is left alone.
        assert_eq!(cmdline("a \"b", &[], true), "a \"b");
    }

    #[test]
    fn coord_rejects_over_i16() {
        let size = |n: u16| WindowSize {
            rows: n,
            cols: n,
            ..WindowSize::default()
        };
        let ok = coord(size(32767)).unwrap();
        assert_eq!((ok.X, ok.Y), (32767, 32767));
        let err = coord(size(32768)).err().unwrap();
        assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
        assert_eq!(err.to_string(), "window size 32768x32768 exceeds 32767");
    }
}
