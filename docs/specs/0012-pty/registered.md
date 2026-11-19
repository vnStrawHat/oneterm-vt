# `Registered`: registration in safe code (added after v0.4.1)

## Why

`EventedReadWrite::register` is `unsafe` because `Poller::add_with_mode` is: a source must be
deleted from the poller before it is dropped. Every embedder wrote the same `unsafe` block and kept
that rule by hand, and a crate that forbids `unsafe` could not run a pty loop at all. The rule can
be kept by construction, inside the engine, where the `unsafe` already is.

## Who does what

| Part | Owner |
|---|---|
| deregister before the console is dropped; keep the poller alive until then | engine (`Registered`) |
| undo a registration that failed half way | engine (`Registered::new`) |
| the loop: wait, drain, pending writes, write interest, resize, threads, `Poller::notify` | embedder |

## Shape (`src/pty/registered.rs`)

```rust
pub struct Registered<C: EventedReadWrite = PseudoConsole> { console: C, poller: Arc<Poller> }

impl<C: EventedReadWrite> Registered<C> {
    pub fn new(console: C, poller: Arc<Poller>, interest: Event, mode: PollMode) -> io::Result<Self>;
    pub fn poller(&self) -> &Arc<Poller>;
    pub fn console(&self) -> &C;                      // `child_pid`
    pub fn reregister(&mut self, interest: Event, mode: PollMode) -> io::Result<()>;
}
impl<C: EventedPty> Registered<C> { pub fn next_child_event(&mut self) -> Option<ChildEvent>; }
impl<C: EventedReadWrite + OnResize> Registered<C> { pub fn on_resize(&mut self, size: WindowSize) -> io::Result<()>; }
impl<C: EventedReadWrite> io::Read for Registered<C>   // console.reader().read
impl<C: EventedReadWrite> io::Write for Registered<C>  // console.writer().write / flush
impl<C: EventedReadWrite> Drop for Registered<C>       // console.deregister(&poller); a failure is logged
```

| Rule | Reason |
|---|---|
| `new` takes the console by value and an `Arc<Poller>` | the registration cannot outlive either; other clones of the `Arc` may outlive it, harmless once deregistered |
| `new` calls `deregister` when `register` failed, then drops the console | an implementation may have added one source before the other failed |
| `drop` deregisters, then the fields drop: console, poller | the rule of `register`; `mem::forget` leaks both, which keeps the rule too |
| no `reader()`, `writer()`, `DerefMut` or `&mut C` | see below |
| `console()` is `&C` | enough for `child_pid`; nothing can be moved out or closed through it |
| generic over `C`, default `PseudoConsole` | the trait's safety rule is the same for every implementation; the loopback fake tests it on every OS without a child |
| inherent `next_child_event` and `on_resize`, not the traits | `EventedPty` needs `register` as a supertrait method; one way to call each |

## Why no `&mut` to the reader or the writer

On Unix the reader is the master `File`, a registered source. `std::mem::swap` on the `&mut File`s of
two registrations is safe code, and after it each registration owns the other's descriptor: dropping
one deletes the wrong descriptor from its poller (an error, ignored) and closes a descriptor the
other poller still holds. That breaks the rule `Registered` exists to keep. An opaque
`&mut impl Read` does not help: two values of one opaque type swap as well. `Read` and `Write` on
the registration give the same calls with nothing to swap.

## Not provided

Taking the console back out (`into_console`). A caller that moves a console between pollers uses
`register` and `deregister`; no embedder asked for it.

## Tests

| Where | Test |
|---|---|
| `pty/loopback_tests.rs` | `a_registration_reads_writes_resizes_and_reports_the_exit` (the registration holds the only `Arc` of the poller) |
| | `a_registration_changes_its_interest` (`reregister` to writable wakes on the RW token) |
| | `a_dropped_registration_deregisters_before_its_console_is_dropped` (fake logs `registered`, `deregistered`, `dropped`) |
| | `a_failed_registration_takes_back_what_it_added` (the fake fails after its first source: `deregistered`, `dropped`) |
| `pty.rs` | `pseudo_console_is_send` also asserts `Registered: Send` |
| `tests/pty_contract.rs` | every contract test runs through `Registered` on a real shell, under `#![forbid(unsafe_code)]` |
| `docs/user/pty.md` | the poll loop doctest, under `#![forbid(unsafe_code)]` |

Versioning: added items only, a `minor` bump.
