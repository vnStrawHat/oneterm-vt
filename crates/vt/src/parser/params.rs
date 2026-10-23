//! Numeric parameters and intermediate bytes of escape, CSI and DCS sequences.

/// Most parameter values (sub-parameters included) kept per sequence; past it the sequence is
/// flagged ignored and the first `MAX_PARAMS` values stay unchanged.
pub const MAX_PARAMS: usize = 32;

/// Most intermediate bytes kept per sequence, a private marker included; past it the sequence is
/// flagged ignored.
pub const MAX_INTERMEDIATES: usize = 2;

/// The separator written before a parameter value.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ParamSep {
    /// `;`, which starts a new parameter. The first value always reports this.
    Semicolon,
    /// `:`, which continues the parameter of the previous value as a sub-parameter.
    Colon,
}

/// The parameters of one control sequence, at most [`MAX_PARAMS`] values.
///
/// An absent value reads as `0`, and a value saturates at `65535`. Only the parser builds it.
#[derive(Clone, Debug)]
pub struct Params {
    values: [u16; MAX_PARAMS],
    seps: [ParamSep; MAX_PARAMS],
    len: u8,
    ignored: bool,
}

impl Params {
    pub(super) fn new() -> Self {
        Self {
            values: [0; MAX_PARAMS],
            seps: [ParamSep::Semicolon; MAX_PARAMS],
            len: 0,
            ignored: false,
        }
    }

    pub(super) fn clear(&mut self) {
        self.len = 0;
        self.ignored = false;
    }

    pub(super) fn is_full(&self) -> bool {
        usize::from(self.len) >= MAX_PARAMS
    }

    pub(super) fn mark_ignored(&mut self) {
        self.ignored = true;
    }

    /// Appends a value; a full list flags the sequence instead.
    pub(super) fn push(&mut self, value: u16, sep: ParamSep) {
        let index = usize::from(self.len);
        let (Some(value_slot), Some(sep_slot)) =
            (self.values.get_mut(index), self.seps.get_mut(index))
        else {
            self.ignored = true;
            return;
        };
        *value_slot = value;
        *sep_slot = sep;
        self.len += 1;
    }

    /// Number of values, sub-parameters included.
    pub fn len(&self) -> usize {
        usize::from(self.len)
    }

    /// Whether no value was collected. Never true inside a `csi` or `dcs_hook` call.
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// The value at a flat index, sub-parameters included.
    pub fn get(&self, index: usize) -> Option<u16> {
        self.values().get(index).copied()
    }

    /// The separator before the value at a flat index; index 0 is [`ParamSep::Semicolon`].
    pub fn sep(&self, index: usize) -> Option<ParamSep> {
        self.seps().get(index).copied()
    }

    /// All values in order, sub-parameters included.
    pub fn values(&self) -> &[u16] {
        self.values.get(..self.len()).unwrap_or_default()
    }

    fn seps(&self) -> &[ParamSep] {
        self.seps.get(..self.len()).unwrap_or_default()
    }

    /// One slice per `;`-separated parameter; the slice holds the value and its `:` sub-parameters.
    pub fn groups(&self) -> ParamGroups<'_> {
        ParamGroups {
            values: self.values(),
            seps: self.seps(),
        }
    }

    /// Whether a limit was hit or the sequence header was malformed. The sink should drop the
    /// sequence.
    pub fn ignored(&self) -> bool {
        self.ignored
    }
}

/// Iterator over the `;`-separated parameters of [`Params`], from [`Params::groups`].
#[derive(Clone, Debug)]
pub struct ParamGroups<'a> {
    values: &'a [u16],
    seps: &'a [ParamSep],
}

impl<'a> Iterator for ParamGroups<'a> {
    type Item = &'a [u16];

    fn next(&mut self) -> Option<Self::Item> {
        if self.values.is_empty() {
            return None;
        }
        let sub_params = self
            .seps
            .iter()
            .skip(1)
            .take_while(|sep| **sep == ParamSep::Colon)
            .count();
        let end = sub_params + 1;
        let (group, rest) = self.values.split_at_checked(end)?;
        self.values = rest;
        self.seps = self.seps.get(end..).unwrap_or_default();
        Some(group)
    }
}

/// Intermediate bytes (`0x20..=0x2F`, plus a private marker `0x3C..=0x3F` first) of a sequence.
#[derive(Clone, Copy, Debug)]
pub struct Intermediates {
    bytes: [u8; MAX_INTERMEDIATES],
    len: u8,
}

impl Intermediates {
    pub(super) fn new() -> Self {
        Self {
            bytes: [0; MAX_INTERMEDIATES],
            len: 0,
        }
    }

    pub(super) fn clear(&mut self) {
        self.len = 0;
    }

    /// Stores a byte; returns `false` when the list is full and the byte was dropped.
    pub(super) fn push(&mut self, byte: u8) -> bool {
        let Some(slot) = self.bytes.get_mut(usize::from(self.len)) else {
            return false;
        };
        *slot = byte;
        self.len += 1;
        true
    }

    /// The bytes in arrival order.
    pub fn as_slice(&self) -> &[u8] {
        self.bytes.get(..usize::from(self.len)).unwrap_or_default()
    }
}
