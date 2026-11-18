//! Mouse pointer shapes: OSC 22 as xterm defined it (one name) with the additions of the
//! kitty pointer-shape protocol (a stack of shapes per screen, and queries).
//!
//! The engine keeps the stacks, answers the queries and reports the shape to show. Drawing a
//! pointer, and which names can be drawn, are the embedder's: it says so in
//! [`PointerShapes`].

use std::borrow::Cow;

use crate::events::VtEvent;
use crate::parser::{OscParams, StringTerm};

use super::dispatch::Handler;

/// Shapes one screen's stack holds; pushing past it drops the oldest.
pub(super) const POINTER_STACK_MAX: usize = 16;
/// Longest shape name in bytes.
pub(super) const POINTER_NAME_MAX: usize = 64;
/// Names one query may ask about; a longer query gets no answer.
pub(super) const POINTER_QUERY_MAX: usize = 64;

/// What OSC 22 queries answer about the embedder's mouse pointers.
///
/// Build it with the struct-update form, `PointerShapes { default_shape: "default".into(),
/// ..PointerShapes::default() }`. Names hold only `a-z 0-9 _ -` and at most 64 bytes: when the
/// terminal is built, a `supported` name that breaks the rule is dropped and a `default_shape`
/// or `grabbed_shape` that breaks it is replaced by this type's default.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct PointerShapes {
    /// The names the embedder can draw. A query for one of them answers `1`, for any other
    /// name `0`. The default is [`PointerShapes::CSS`]. Setting a shape is not checked against
    /// this list: a name outside it still arrives as [`VtEvent::Pointer`].
    pub supported: Vec<Cow<'static, str>>,
    /// The answer to `?__default__`: the pointer shown while no program named one (default
    /// `text`).
    pub default_shape: Cow<'static, str>,
    /// The answer to `?__grabbed__`: the pointer shown while a program reports the mouse
    /// (default `default`, the arrow).
    pub grabbed_shape: Cow<'static, str>,
}

impl PointerShapes {
    /// The shape names every terminal that implements the protocol is asked to support: the
    /// CSS cursor names. A slice, so that a name added later changes no type.
    pub const CSS: &'static [&'static str] = &[
        "alias",
        "cell",
        "copy",
        "crosshair",
        "default",
        "e-resize",
        "ew-resize",
        "grab",
        "grabbing",
        "help",
        "move",
        "n-resize",
        "ne-resize",
        "nesw-resize",
        "no-drop",
        "not-allowed",
        "ns-resize",
        "nw-resize",
        "nwse-resize",
        "pointer",
        "progress",
        "s-resize",
        "se-resize",
        "sw-resize",
        "text",
        "vertical-text",
        "w-resize",
        "wait",
        "zoom-in",
        "zoom-out",
    ];

    /// The same answers with every name that cannot go into a reply removed or replaced.
    pub(super) fn cleaned(mut self) -> PointerShapes {
        let defaults = PointerShapes::default();
        self.supported.retain(|name| is_shape_name(name));
        if !is_shape_name(&self.default_shape) {
            self.default_shape = defaults.default_shape;
        }
        if !is_shape_name(&self.grabbed_shape) {
            self.grabbed_shape = defaults.grabbed_shape;
        }
        self
    }
}

impl Default for PointerShapes {
    fn default() -> Self {
        PointerShapes {
            supported: PointerShapes::CSS
                .iter()
                .copied()
                .map(Cow::Borrowed)
                .collect(),
            default_shape: Cow::Borrowed("text"),
            grabbed_shape: Cow::Borrowed("default"),
        }
    }
}

/// Whether `name` may be a shape name: `a-z 0-9 _ -`, not empty, at most 64 bytes.
fn is_shape_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= POINTER_NAME_MAX
        && name
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || b"_-".contains(&byte))
}

/// The shapes a program stacked on one screen. An empty name is a level that asks for the
/// terminal's own pointer: `OSC 22 ;` replaces the top, it does not pop it.
#[derive(Debug, Default)]
pub(super) struct PointerStack {
    shapes: Vec<String>,
}

impl PointerStack {
    /// The shape to show, or `None` for the terminal's own.
    pub(super) fn current(&self) -> Option<&str> {
        self.shapes
            .last()
            .map(String::as_str)
            .filter(|name| !name.is_empty())
    }

    /// `OSC 22 ; name`: replaces the top; on an empty stack a name becomes its only level.
    fn set(&mut self, name: &str) {
        match self.shapes.last_mut() {
            Some(top) => name.clone_into(top),
            None if !name.is_empty() => self.shapes.push(name.to_owned()),
            None => {}
        }
    }

    /// `OSC 22 ; > name`: a new level; at the cap the oldest goes.
    fn push(&mut self, name: &str) {
        if self.shapes.len() >= POINTER_STACK_MAX {
            self.shapes.remove(0);
        }
        self.shapes.push(name.to_owned());
    }

    /// `OSC 22 ; <`: drops the top level; `false` when there was none.
    fn pop(&mut self) -> bool {
        self.shapes.pop().is_some()
    }
}

/// The shape stacks of the two screens; the active one decides the pointer.
#[derive(Debug, Default)]
pub(super) struct PointerStacks {
    pub(super) active: PointerStack,
    inactive: PointerStack,
}

impl PointerStacks {
    /// Called on every real switch between the screens.
    pub(super) fn swap(&mut self) {
        std::mem::swap(&mut self.active, &mut self.inactive);
    }
}

impl Handler<'_> {
    /// OSC 22: `[= > < ?] name,name,..`. Without a first character it is `=`.
    pub(super) fn osc_pointer(&mut self, params: &OscParams<'_>, term: StringTerm) {
        let Some(Ok(text)) = params.get(1).map(std::str::from_utf8) else {
            self.unhandled();
            return;
        };
        let (operation, names) = match text.as_bytes().first() {
            Some(first @ (b'=' | b'>' | b'<' | b'?')) => {
                (*first, text.get(1..).unwrap_or_default())
            }
            _ => (b'=', text),
        };
        match operation {
            b'?' => self.answer_pointer_query(names, term),
            b'<' => {
                // The names after `<` mean nothing; a pop of an empty stack does nothing.
                if self.state.pointer.active.pop() {
                    self.push_pointer();
                }
            }
            b'=' if names.is_empty() => {
                self.state.pointer.active.set("");
                self.push_pointer();
            }
            _ => self.name_pointer(operation, names),
        }
    }

    /// Sets (`=`) or pushes (`>`) the names of a list. One name that is no shape name refuses
    /// the whole sequence.
    fn name_pointer(&mut self, operation: u8, names: &str) {
        if !names.split(',').all(is_shape_name) {
            self.unhandled();
            return;
        }
        let stack = &mut self.state.pointer.active;
        for name in names.split(',') {
            if operation == b'>' {
                stack.push(name);
            } else {
                // The protocol names one shape here; of a list the last one stays, as the
                // last pushed name is the one shown.
                stack.set(name);
            }
        }
        self.push_pointer();
    }

    /// The answer to `OSC 22 ; ? name,..`: one value per name, in order.
    fn answer_pointer_query(&mut self, names: &str, term: StringTerm) {
        if names.is_empty() || names.split(',').count() > POINTER_QUERY_MAX {
            self.unhandled();
            return;
        }
        let state = &mut *self.state;
        let shapes = &state.config.pointer_shapes;
        let scratch = &mut state.scratch;
        scratch.clear();
        scratch.push_str("\x1b]22;");
        for (index, name) in names.split(',').enumerate() {
            if index > 0 {
                scratch.push(',');
            }
            scratch.push_str(match name {
                "__current__" => state.pointer.active.current().unwrap_or("0"),
                "__default__" => shapes.default_shape.as_ref(),
                "__grabbed__" => shapes.grabbed_shape.as_ref(),
                name if shapes.supported.iter().any(|known| known.as_ref() == name) => "1",
                _ => "0",
            });
        }
        scratch.push_str(match term {
            StringTerm::Bel => "\x07",
            StringTerm::St => "\x1b\\",
        });
        self.out.push_reply(scratch.as_bytes());
    }

    /// Reports the shape to show now.
    pub(super) fn push_pointer(&mut self) {
        match self.state.pointer.active.current() {
            Some(name) => {
                let span = self.out.text_span(name);
                self.out.push(VtEvent::Pointer(span));
            }
            None => self.out.push(VtEvent::PointerReset),
        }
    }

    /// After a switch between the screens: reports the shape of the screen now shown when it
    /// differs from the one that was shown.
    pub(super) fn note_pointer_switch(&mut self) {
        let pointer = &self.state.pointer;
        if pointer.active.current() != pointer.inactive.current() {
            self.push_pointer();
        }
    }
}
