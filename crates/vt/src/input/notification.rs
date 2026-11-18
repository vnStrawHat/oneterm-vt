//! What an embedder tells a program about a notification it sent with OSC 99.

/// A report about an OSC 99 notification, for the program that sent it.
///
/// The engine cannot know what the user did with a notification, so the embedder builds the
/// report and writes [`encode`](Self::encode) to the program. Send one only when the
/// notification asked for it (see [`Notification`](crate::Notification)) and when you
/// declared the ability in [`NotificationSupport`](crate::NotificationSupport).
///
/// `id` is [`Notification::id`](crate::Notification::id). It is written without any character
/// an id may not hold, and as `0` when empty, which is what the protocol asks for a
/// notification without an id.
///
/// ```
/// use oneterm_vt::input::NotificationReport;
///
/// let clicked = NotificationReport::Activated { id: "build-1" };
/// assert_eq!(clicked.encode(), b"\x1b]99;i=build-1;\x1b\\");
/// let second_button = NotificationReport::Button { id: "build-1", button: 2 };
/// assert_eq!(second_button.encode(), b"\x1b]99;i=build-1;2\x1b\\");
/// let closed = NotificationReport::Closed { id: "" };
/// assert_eq!(closed.encode(), b"\x1b]99;i=0:p=close;\x1b\\");
/// ```
#[non_exhaustive]
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum NotificationReport<'a> {
    /// The user activated the notification as a whole. For a notification with
    /// `report_activation`.
    Activated {
        /// The notification's id.
        id: &'a str,
    },
    /// The user pressed one of its buttons. For a notification with `report_activation`.
    Button {
        /// The notification's id.
        id: &'a str,
        /// Which button, counted from 1 in the order of `buttons`.
        button: u32,
    },
    /// The notification was closed. For a notification with `report_close`.
    Closed {
        /// The notification's id.
        id: &'a str,
    },
    /// The platform does not say when a notification closes. Sent instead of
    /// [`Closed`](Self::Closed), once, when a notification with `report_close` is shown; the
    /// program then asks with `p=alive`.
    CloseUntracked {
        /// The notification's id.
        id: &'a str,
    },
    /// The answer to [`VtEvent::NotificationAliveQuery`](crate::VtEvent::NotificationAliveQuery).
    Alive {
        /// The id of the query.
        id: &'a str,
        /// The ids of the notifications of this terminal that are still shown.
        alive: &'a [&'a str],
    },
}

impl NotificationReport<'_> {
    /// The bytes to write to the program.
    pub fn encode(&self) -> Vec<u8> {
        let (id, kind, payload) = match *self {
            NotificationReport::Activated { id } => (id, "", String::new()),
            NotificationReport::Button { id, button } => (id, "", button.to_string()),
            NotificationReport::Closed { id } => (id, ":p=close", String::new()),
            NotificationReport::CloseUntracked { id } => (id, ":p=close", "untracked".to_owned()),
            NotificationReport::Alive { id, alive } => {
                let ids: Vec<String> = alive
                    .iter()
                    .map(|id| clean_id(id.as_bytes()))
                    .filter(|id| !id.is_empty())
                    .collect();
                (id, ":p=alive", ids.join(","))
            }
        };
        let id = clean_id(id.as_bytes());
        let id = if id.is_empty() { "0" } else { id.as_str() };
        format!("\x1b]99;i={id}{kind};{payload}\x1b\\").into_bytes()
    }
}

/// `id` without the bytes an identifier may not hold (`a-z A-Z 0-9 _ - + .` stay): ids come
/// from programs and go back in replies, so nothing else may get into one.
pub(crate) fn clean_id(id: &[u8]) -> String {
    id.iter()
        .filter(|byte| byte.is_ascii_alphanumeric() || b"_-+.".contains(byte))
        .map(|byte| char::from(*byte))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(report: NotificationReport<'_>) -> String {
        String::from_utf8(report.encode()).expect("ASCII")
    }

    #[test]
    fn each_report_has_its_bytes() {
        let id = "n-1";
        assert_eq!(
            text(NotificationReport::Activated { id }),
            "\x1b]99;i=n-1;\x1b\\"
        );
        assert_eq!(
            text(NotificationReport::Button { id, button: 3 }),
            "\x1b]99;i=n-1;3\x1b\\"
        );
        assert_eq!(
            text(NotificationReport::Closed { id }),
            "\x1b]99;i=n-1:p=close;\x1b\\"
        );
        assert_eq!(
            text(NotificationReport::CloseUntracked { id }),
            "\x1b]99;i=n-1:p=close;untracked\x1b\\"
        );
        assert_eq!(
            text(NotificationReport::Alive {
                id,
                alive: &["a", "b.2"]
            }),
            "\x1b]99;i=n-1:p=alive;a,b.2\x1b\\"
        );
        assert_eq!(
            text(NotificationReport::Alive { id, alive: &[] }),
            "\x1b]99;i=n-1:p=alive;\x1b\\"
        );
    }

    #[test]
    fn ids_are_written_clean_and_an_empty_one_as_zero() {
        assert_eq!(
            text(NotificationReport::Activated {
                id: "a\x1b]b;c:d=e\x07"
            }),
            "\x1b]99;i=abcde;\x1b\\"
        );
        assert_eq!(
            text(NotificationReport::Activated { id: "" }),
            "\x1b]99;i=0;\x1b\\"
        );
        assert_eq!(
            text(NotificationReport::Closed { id: "\x1b" }),
            "\x1b]99;i=0:p=close;\x1b\\"
        );
        assert_eq!(
            text(NotificationReport::Alive {
                id: "q",
                alive: &["ok", "", "\x07", "x,y"]
            }),
            "\x1b]99;i=q:p=alive;ok,xy\x1b\\"
        );
    }
}
