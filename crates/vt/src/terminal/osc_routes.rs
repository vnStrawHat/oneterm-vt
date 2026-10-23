//! Which OSC numbers run built in, are forwarded to the embedder, or are dropped.

/// What happens to an OSC number.
#[non_exhaustive]
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum OscRoute {
    /// The built-in handler runs; nothing is forwarded. The default for a number in
    /// [`OscRoutes::BUILTIN`].
    Builtin,
    /// The built-in handler runs first, then a [`VtEvent::Osc`](crate::VtEvent::Osc) follows.
    BuiltinAndForward,
    /// Only a [`VtEvent::Osc`](crate::VtEvent::Osc) is pushed.
    Forward,
    /// Nothing runs and nothing is pushed; the OSC is counted as unhandled. The default for
    /// every number outside [`OscRoutes::BUILTIN`].
    Drop,
}

/// Codes below this live in a bitmap; the rest in a sorted list.
const BITMAP_CODES: u32 = 2048;

/// A set of OSC numbers: a bitmap for `0..2048` and a sorted list above it.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
struct CodeSet {
    low: [u64; 32],
    high: Vec<u32>,
}

impl CodeSet {
    fn contains(&self, code: u32) -> bool {
        if code < BITMAP_CODES {
            self.low
                .get((code / 64) as usize)
                .is_some_and(|word| word & (1 << (code % 64)) != 0)
        } else {
            self.high.binary_search(&code).is_ok()
        }
    }

    fn set(&mut self, code: u32, on: bool) {
        if code < BITMAP_CODES {
            if let Some(word) = self.low.get_mut((code / 64) as usize) {
                if on {
                    *word |= 1 << (code % 64);
                } else {
                    *word &= !(1 << (code % 64));
                }
            }
            return;
        }
        match (self.high.binary_search(&code), on) {
            (Err(at), true) => self.high.insert(at, code),
            (Ok(at), false) => {
                self.high.remove(at);
            }
            _ => {}
        }
    }

    /// Every member, ascending.
    fn members(&self) -> impl Iterator<Item = u32> + '_ {
        let low = self.low.iter().enumerate().flat_map(|(index, word)| {
            (0..64u32)
                .filter(move |bit| word & (1 << bit) != 0)
                .map(move |bit| index as u32 * 64 + bit)
        });
        low.chain(self.high.iter().copied())
    }
}

/// The built-in handlers as a bitmap, so [`OscRoutes::has_builtin`] is a lookup.
const BUILTIN_SET: [u64; 32] = {
    let mut words = [0u64; 32];
    let mut index = 0;
    while index < OscRoutes::BUILTIN.len() {
        let code = OscRoutes::BUILTIN[index];
        words[(code / 64) as usize] |= 1 << (code % 64);
        index += 1;
    }
    words
};

/// The route of every OSC number.
///
/// Every number in [`BUILTIN`](Self::BUILTIN) runs its built-in handler; every other number is
/// dropped and counted as unhandled. [`route`](Self::route) changes that per number.
/// `route` stores the route a number will actually get, so two tables that behave the same
/// compare equal. The table is read when the [`Terminal`](crate::Terminal) is built.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct OscRoutes {
    forward: CodeSet,
    suppress: CodeSet,
    large: CodeSet,
}

impl OscRoutes {
    /// The OSC numbers with a built-in handler.
    pub const BUILTIN: [u32; 20] = [
        0, 1, 2, 4, 7, 8, 9, 10, 11, 12, 17, 19, 22, 50, 52, 104, 110, 111, 112, 133,
    ];

    /// Every built-in runs, nothing is forwarded.
    pub fn new() -> OscRoutes {
        OscRoutes::default()
    }

    /// Whether `code` has a built-in handler.
    pub fn has_builtin(code: u32) -> bool {
        code < BITMAP_CODES
            && BUILTIN_SET
                .get((code / 64) as usize)
                .is_some_and(|word| word & (1 << (code % 64)) != 0)
    }

    /// Sets the route of `code`.
    ///
    /// Asking for `Builtin` or `BuiltinAndForward` on a number without a built-in handler
    /// reads back as `Drop` or `Forward`: there is nothing to run.
    pub fn route(&mut self, code: u32, route: OscRoute) -> &mut Self {
        let builtin = OscRoutes::has_builtin(code);
        let (forward, suppress) = match route {
            OscRoute::Builtin => (false, false),
            OscRoute::BuiltinAndForward => (true, !builtin),
            OscRoute::Forward => (true, true),
            OscRoute::Drop => (false, builtin),
        };
        self.forward.set(code, forward);
        self.suppress.set(code, suppress);
        self
    }

    /// [`route`](Self::route) for every number in `codes`.
    pub fn route_all(&mut self, codes: &[u32], route: OscRoute) -> &mut Self {
        for code in codes {
            self.route(*code, route);
        }
        self
    }

    /// Every number whose route differs from its default, ascending, with that route. For
    /// diagnostics; it allocates.
    pub fn overrides(&self) -> impl Iterator<Item = (u32, OscRoute)> + '_ {
        let mut codes: Vec<u32> = self
            .forward
            .members()
            .chain(self.suppress.members())
            .collect();
        codes.sort_unstable();
        codes.dedup();
        codes.into_iter().filter_map(|code| {
            let default = if OscRoutes::has_builtin(code) {
                OscRoute::Builtin
            } else {
                OscRoute::Drop
            };
            let route = self.get(code);
            (route != default).then_some((code, route))
        })
    }

    /// Lets the parser hold up to `OSC_LARGE` (8 MiB) of payload for `code` instead of
    /// `OSC_INLINE` (2 KiB). A memory ceiling only: it applies while the route is not `Drop`.
    pub fn large(&mut self, code: u32, allow: bool) -> &mut Self {
        self.large.set(code, allow);
        self
    }

    /// The route of `code`.
    pub fn get(&self, code: u32) -> OscRoute {
        let builtin = OscRoutes::has_builtin(code);
        match (self.forward.contains(code), self.suppress.contains(code)) {
            (false, false) if builtin => OscRoute::Builtin,
            (true, false) if builtin => OscRoute::BuiltinAndForward,
            (true, _) => OscRoute::Forward,
            _ => OscRoute::Drop,
        }
    }

    /// Whether [`large`](Self::large) was set for `code`.
    pub fn allows_large(&self, code: u32) -> bool {
        self.large.contains(code)
    }
}
