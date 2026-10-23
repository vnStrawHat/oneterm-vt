//! Operating system command accumulation: the parser's only heap allocation.

/// Most OSC parameters reported. The last one holds the rest of the payload, `;` included.
pub const MAX_OSC_PARAMS: usize = 16;

/// OSC payload bytes held without growing; past it the payload is truncated unless the sink
/// claimed the code as large.
pub const OSC_INLINE: usize = 2 * 1024;

/// OSC payload ceiling for codes the sink claims through `Dispatch::osc_allows_large`; past it
/// the payload is truncated.
pub const OSC_LARGE: usize = 8 * 1024 * 1024;

/// How an OSC string ended.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StringTerm {
    /// Terminated by `BEL`.
    Bel,
    /// Terminated by `ESC`; the `\` that usually follows arrives as a separate `esc` call.
    St,
}

/// The `;`-separated parameters of one OSC, borrowed from the parser for the length of the call.
#[derive(Clone, Copy, Debug)]
pub struct OscParams<'a> {
    payload: &'a [u8],
    bounds: &'a [(u32, u32)],
}

impl<'a> OscParams<'a> {
    /// Number of parameters, at least 1 and at most [`MAX_OSC_PARAMS`].
    pub fn len(&self) -> usize {
        self.bounds.len()
    }

    /// Whether there is no parameter. Never true inside an `osc` call.
    pub fn is_empty(&self) -> bool {
        self.bounds.is_empty()
    }

    /// The bytes of one parameter.
    pub fn get(&self, index: usize) -> Option<&'a [u8]> {
        let &(start, end) = self.bounds.get(index)?;
        self.payload.get(start as usize..end as usize)
    }

    /// All parameters in order.
    pub fn iter(&self) -> impl Iterator<Item = &'a [u8]> + '_ {
        (0..self.len()).filter_map(|index| self.get(index))
    }
}

pub(super) struct OscBuffer {
    /// Parameters back to back; the separating `;` is not stored.
    payload: Vec<u8>,
    bounds: [(u32, u32); MAX_OSC_PARAMS],
    count: usize,
    param_start: u32,
    code: Option<u32>,
    cap: usize,
    truncated: bool,
}

impl OscBuffer {
    pub(super) fn new() -> Self {
        Self {
            payload: Vec::with_capacity(OSC_INLINE),
            bounds: [(0, 0); MAX_OSC_PARAMS],
            count: 0,
            param_start: 0,
            code: None,
            cap: OSC_INLINE,
            truncated: false,
        }
    }

    /// Empties the buffer and gives back memory a large OSC grew.
    pub(super) fn start(&mut self) {
        self.payload.clear();
        self.count = 0;
        self.param_start = 0;
        self.code = None;
        self.cap = OSC_INLINE;
        self.truncated = false;
        if self.payload.capacity() > OSC_INLINE {
            self.payload.shrink_to(OSC_INLINE);
        }
    }

    pub(super) fn code(&self) -> Option<u32> {
        self.code
    }

    pub(super) fn truncated(&self) -> bool {
        self.truncated
    }

    pub(super) fn params(&self) -> OscParams<'_> {
        OscParams {
            payload: &self.payload,
            bounds: self.bounds.get(..self.count).unwrap_or_default(),
        }
    }

    pub(super) fn push(&mut self, byte: u8) {
        let len = self.payload.len();
        if len >= self.cap {
            self.truncated = true;
            return;
        }
        if len == self.payload.capacity() {
            // Doubling, never past the cap, so capacity stays within `OSC_LARGE`.
            self.payload.reserve_exact(len.min(self.cap - len));
        }
        self.payload.push(byte);
    }

    /// Handles a `;`. Returns `true` when it closed the first parameter.
    pub(super) fn semicolon(&mut self) -> bool {
        if self.truncated || self.count + 1 >= MAX_OSC_PARAMS {
            // The last parameter keeps its `;` so a sink can split it again.
            self.push(b';');
            return false;
        }
        self.close_param()
    }

    /// Closes the open parameter. Returns `true` when it was the first one.
    pub(super) fn close_param(&mut self) -> bool {
        let end = u32::try_from(self.payload.len()).unwrap_or(u32::MAX);
        let was_first = self.count == 0;
        if let Some(slot) = self.bounds.get_mut(self.count) {
            *slot = (self.param_start, end);
            self.count += 1;
        }
        self.param_start = end;
        was_first
    }

    /// Reads the code from the first parameter and, if `claims_large` says so for it, lifts the cap.
    pub(super) fn resolve_code(&mut self, claims_large: impl FnOnce(u32) -> bool) {
        let first = self
            .params()
            .get(0)
            .filter(|bytes| !bytes.is_empty() && bytes.iter().all(u8::is_ascii_digit));
        self.code = first
            .and_then(|bytes| std::str::from_utf8(bytes).ok())
            .and_then(|digits| digits.parse().ok());
        if let Some(code) = self.code
            && claims_large(code)
        {
            self.cap = OSC_LARGE;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fill(buffer: &mut OscBuffer, bytes: usize) {
        for _ in 0..bytes {
            buffer.push(b'A');
        }
    }

    #[test]
    fn unclaimed_osc_never_grows() {
        let mut buffer = OscBuffer::new();
        buffer.start();
        buffer.push(b'0');
        buffer.semicolon();
        for _ in 0..(20 * 16) {
            fill(&mut buffer, 64 * 1024);
            assert_eq!(buffer.payload.capacity(), OSC_INLINE);
        }
        assert!(buffer.truncated());
        assert_eq!(buffer.payload.len(), OSC_INLINE);
    }

    #[test]
    fn claimed_osc_grows_to_large_and_shrinks_back() {
        let mut buffer = OscBuffer::new();
        buffer.start();
        buffer.push(b'5');
        buffer.push(b'2');
        assert!(buffer.semicolon());
        buffer.resolve_code(|code| code == 52);
        assert_eq!(buffer.code(), Some(52));
        for _ in 0..9 * 16 {
            fill(&mut buffer, 64 * 1024);
        }
        assert_eq!(buffer.payload.len(), OSC_LARGE);
        assert!(buffer.payload.capacity() <= OSC_LARGE);
        assert!(buffer.truncated());
        buffer.start();
        assert_eq!(buffer.payload.capacity(), OSC_INLINE);
        assert!(!buffer.truncated());
    }

    #[test]
    fn semicolons_stop_closing_after_the_last_param() {
        let mut buffer = OscBuffer::new();
        buffer.start();
        for _ in 0..40 {
            buffer.push(b'x');
            buffer.semicolon();
        }
        buffer.close_param();
        assert_eq!(buffer.params().len(), MAX_OSC_PARAMS);
    }
}
