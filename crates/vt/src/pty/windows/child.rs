//! Child exit as a poll event: a thread-pool wait on the process handle.

#![expect(
    unsafe_code,
    reason = "Win32 FFI: process wait callback and termination"
)]

use std::ffi::c_void;
use std::io;
use std::os::windows::io::{AsRawHandle, OwnedHandle};
use std::os::windows::process::ExitStatusExt;
use std::process::ExitStatus;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError, mpsc};

use polling::os::iocp::{CompletionPacket, PollerIocpExt};
use polling::{Event, Poller};
use windows_sys::Win32::Foundation::{HANDLE, INVALID_HANDLE_VALUE, WAIT_OBJECT_0};
use windows_sys::Win32::System::Threading::{
    GetExitCodeProcess, GetProcessId, INFINITE, RegisterWaitForSingleObject, TerminateProcess,
    UnregisterWaitEx, WT_EXECUTEINWAITTHREAD, WT_EXECUTEONLYONCE, WaitForSingleObject,
};

use crate::pty::ChildEvent;

/// How long `drop` lets the child leave after the console closed: a started client exits
/// within milliseconds of the close request, one still initialising may never see it.
const CHILD_EXIT_GRACE: u32 = 2000;

struct Registration {
    poller: Arc<Poller>,
    packet: CompletionPacket,
}

#[derive(Default)]
struct Notify {
    exited: bool,
    /// One wake per registration: set when the packet is posted, cleared by `deregister`.
    posted: bool,
    registration: Option<Registration>,
}

impl Notify {
    fn post_if_owed(&mut self) {
        if !self.exited || self.posted {
            return;
        }
        let Some(registration) = &self.registration else {
            return;
        };
        match registration.poller.post(registration.packet.clone()) {
            Ok(()) => self.posted = true,
            Err(cause) => log::warn!("pty: cannot post the child exit packet: {cause}"),
        }
    }
}

/// What the wait callback needs. Shared with the thread pool through a raw pointer, so it stays
/// alive until after `UnregisterWaitEx`.
struct ChildExitSender {
    /// The raw process handle; owned by `ChildExitWatcher`, which closes it last.
    process: isize,
    events: mpsc::Sender<ChildEvent>,
    notify: Mutex<Notify>,
}

impl ChildExitSender {
    fn lock(&self) -> MutexGuard<'_, Notify> {
        self.notify.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn report_exit(&self) {
        let mut code = 0u32;
        // SAFETY: the process handle is valid until the watcher is dropped, which waits for
        // this callback first; `code` is a live out-parameter.
        let read = unsafe { GetExitCodeProcess(self.process as HANDLE, &mut code) };
        let status = (read != 0).then(|| ExitStatus::from_raw(code));
        // The event is queued before the packet is posted, so a poll woken by the packet always
        // finds it.
        let _ = self.events.send(ChildEvent::Exited(status));
        let mut notify = self.lock();
        notify.exited = true;
        notify.post_if_owed();
    }
}

/// Watches the child's process handle and posts one packet per registration when it exits.
pub(super) struct ChildExitWatcher {
    wait: isize,
    sender: Arc<ChildExitSender>,
    events: mpsc::Receiver<ChildEvent>,
    /// Closed last, after the wait is unregistered and the grace period is over.
    process: OwnedHandle,
}

unsafe extern "system" fn child_exited(context: *mut c_void, _timed_out: bool) {
    // SAFETY: `context` is `Arc::as_ptr` of the sender, which `ChildExitWatcher` keeps alive
    // until `UnregisterWaitEx(.., INVALID_HANDLE_VALUE)` returned, and that call waits for a
    // running callback.
    let sender = unsafe { &*context.cast::<ChildExitSender>() };
    sender.report_exit();
}

impl ChildExitWatcher {
    /// Starts watching `process`. It fires at once for a child that already exited.
    pub(super) fn new(process: OwnedHandle) -> io::Result<Self> {
        let (events_tx, events) = mpsc::channel();
        let sender = Arc::new(ChildExitSender {
            process: process.as_raw_handle() as isize,
            events: events_tx,
            notify: Mutex::new(Notify::default()),
        });
        let mut wait: HANDLE = std::ptr::null_mut();
        // SAFETY: `process` is a live process handle that outlives the wait (closed after
        // `UnregisterWaitEx`); `child_exited` has the required signature and its context is
        // the `Arc` payload, kept alive by the watcher; `wait` is a live out-parameter.
        let registered = unsafe {
            RegisterWaitForSingleObject(
                &mut wait,
                process.as_raw_handle(),
                Some(child_exited),
                Arc::as_ptr(&sender).cast::<c_void>(),
                INFINITE,
                WT_EXECUTEINWAITTHREAD | WT_EXECUTEONLYONCE,
            )
        };
        if registered == 0 {
            let cause = io::Error::last_os_error();
            return Err(io::Error::new(
                cause.kind(),
                format!("RegisterWaitForSingleObject failed: {cause}"),
            ));
        }
        Ok(Self {
            wait: wait as isize,
            sender,
            events,
            process,
        })
    }

    pub(super) fn pid(&self) -> Option<u32> {
        // SAFETY: the process handle is owned by `self`.
        let pid = unsafe { GetProcessId(self.process.as_raw_handle()) };
        (pid != 0).then_some(pid)
    }

    pub(super) fn next_event(&mut self) -> Option<ChildEvent> {
        self.events.try_recv().ok()
    }

    /// Records where to post. Posts at once when the exit is already known and this
    /// registration was not woken yet.
    pub(super) fn register(&self, poller: &Arc<Poller>, event: Event) {
        let mut notify = self.sender.lock();
        notify.registration = Some(Registration {
            poller: Arc::clone(poller),
            packet: CompletionPacket::new(event),
        });
        notify.post_if_owed();
    }

    /// The next registration is owed a wake again.
    pub(super) fn deregister(&self) {
        let mut notify = self.sender.lock();
        notify.registration = None;
        notify.posted = false;
    }
}

impl Drop for ChildExitWatcher {
    fn drop(&mut self) {
        // SAFETY: `wait` came from `RegisterWaitForSingleObject`; `INVALID_HANDLE_VALUE` makes
        // the call wait for a running callback, after which `sender` may be freed. When the call
        // fails that wait is not guaranteed, so the branch below leaks one `Arc` count instead.
        let unregistered = unsafe { UnregisterWaitEx(self.wait as HANDLE, INVALID_HANDLE_VALUE) };
        if unregistered == 0 {
            // A late callback may still read the context: keep `sender` alive forever.
            std::mem::forget(Arc::clone(&self.sender));
            log::warn!(
                "pty: UnregisterWaitEx failed: {}",
                io::Error::last_os_error()
            );
        }
        let process = self.process.as_raw_handle();
        // SAFETY: the process handle is owned by `self` and closed only after this block.
        unsafe {
            if WaitForSingleObject(process, CHILD_EXIT_GRACE) != WAIT_OBJECT_0 {
                log::warn!("pty: the child did not exit after the console closed; terminating it");
                if TerminateProcess(process, 1) == 0 {
                    log::warn!(
                        "pty: TerminateProcess failed: {}",
                        io::Error::last_os_error()
                    );
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::process::Command;
    use std::time::Duration;

    use polling::Events;

    use super::*;
    use crate::pty::PTY_CHILD_EVENT_TOKEN;

    const WAIT: Duration = Duration::from_secs(15);

    fn child_exiting_with(code: u32) -> OwnedHandle {
        let child = Command::new("cmd.exe")
            .args(["/d", "/c", &format!("exit {code}")])
            .spawn()
            .unwrap();
        OwnedHandle::from(child)
    }

    fn poller() -> Arc<Poller> {
        Arc::new(Poller::new().unwrap())
    }

    /// Waits up to `first` for a wake, then counts them until `quiet` passes without one.
    fn wakes(poller: &Poller, first: Duration, quiet: Duration) -> usize {
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
                .filter(|event| event.key == PTY_CHILD_EVENT_TOKEN)
                .count();
        }
    }

    fn registered(watcher: &ChildExitWatcher, poller: &Arc<Poller>) {
        watcher.register(poller, Event::readable(PTY_CHILD_EVENT_TOKEN));
    }

    #[test]
    fn exit_reported_once_with_code() {
        let poller = poller();
        let mut watcher = ChildExitWatcher::new(child_exiting_with(7)).unwrap();
        registered(&watcher, &poller);
        assert_eq!(
            wakes(&poller, Duration::from_secs(5), Duration::from_millis(300)),
            1
        );
        let Some(ChildEvent::Exited(status)) = watcher.next_event() else {
            panic!("no exit event");
        };
        assert_eq!(status.and_then(|status| status.code()), Some(7));
        assert_eq!(watcher.next_event(), None);
    }

    #[test]
    fn instant_exit_not_missed() {
        let poller = poller();
        let mut watcher = ChildExitWatcher::new(child_exiting_with(0)).unwrap();
        registered(&watcher, &poller);
        assert!(wakes(&poller, Duration::from_secs(5), Duration::from_millis(100)) >= 1);
        assert!(watcher.next_event().is_some());
    }

    #[test]
    fn exit_before_registration_still_wakes() {
        let mut watcher = ChildExitWatcher::new(child_exiting_with(0)).unwrap();
        // Let the exit callback run before anyone listens.
        let deadline = std::time::Instant::now() + WAIT;
        while !watcher.sender.lock().exited {
            assert!(
                std::time::Instant::now() < deadline,
                "the child never exited"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
        let poller = poller();
        registered(&watcher, &poller);
        assert_eq!(
            wakes(&poller, Duration::from_secs(5), Duration::from_millis(300)),
            1
        );
        assert!(watcher.next_event().is_some());
    }

    #[test]
    fn exit_queued_before_recorded() {
        let poller = poller();
        for _ in 0..30 {
            let mut watcher = ChildExitWatcher::new(child_exiting_with(0)).unwrap();
            registered(&watcher, &poller);
            assert!(wakes(&poller, Duration::from_secs(5), Duration::from_millis(100)) >= 1);
            assert!(
                watcher.next_event().is_some(),
                "the packet arrived before the event"
            );
        }
    }

    #[test]
    fn one_wake_per_registration() {
        let poller = poller();
        let watcher = ChildExitWatcher::new(child_exiting_with(0)).unwrap();
        registered(&watcher, &poller);
        assert_eq!(
            wakes(&poller, Duration::from_secs(5), Duration::from_millis(300)),
            1
        );
        // Re-registering to toggle write interest must not wake again.
        registered(&watcher, &poller);
        assert_eq!(
            wakes(
                &poller,
                Duration::from_millis(300),
                Duration::from_millis(300)
            ),
            0
        );
        // After a deregister the next registration is owed one.
        watcher.deregister();
        registered(&watcher, &poller);
        assert_eq!(
            wakes(&poller, Duration::from_secs(5), Duration::from_millis(300)),
            1
        );
    }

    #[test]
    fn pid_available() {
        let watcher = ChildExitWatcher::new(child_exiting_with(0)).unwrap();
        assert!(watcher.pid().is_some_and(|pid| pid > 0));
    }

    #[test]
    fn deregistered_exit_still_observable() {
        let poller = poller();
        let mut watcher = ChildExitWatcher::new(child_exiting_with(5)).unwrap();
        registered(&watcher, &poller);
        watcher.deregister();
        let deadline = std::time::Instant::now() + WAIT;
        let status = loop {
            if let Some(ChildEvent::Exited(status)) = watcher.next_event() {
                break status;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "the child never exited"
            );
            std::thread::sleep(Duration::from_millis(10));
        };
        assert_eq!(status.and_then(|status| status.code()), Some(5));
    }
}
