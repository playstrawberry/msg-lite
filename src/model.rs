//! What a private message may be, and who `/r` answers.
use std::collections::BTreeMap;

/// A player: the two halves of his UUID.
pub type Id = (u64, u64);
/// The longest private message, in characters (a chat line's length).
pub const MAX_MESSAGE: usize = 256;
/// The longest name that is looked up, in characters.
pub const MAX_NAME: usize = 32;
/// While a player wrote to his partner within this time, a message from somebody else does not change who his
/// `/r` answers.
pub const REPLY_WINDOW_MS: u64 = 180_000;
/// The most reply partners remembered (one per online player).
pub const MAX_REPLIES: usize = 4096;

/// The message as it is delivered: trimmed, without control characters and without the section sign, so it is
/// shown as the plain text the player typed.
pub fn message(raw: &str) -> Result<String, &'static str> {
    let text: String = raw
        .chars()
        .filter(|c| !c.is_control() && *c != '\u{a7}')
        .collect();
    let text = text.trim();
    if text.is_empty() {
        Err("empty")
    } else if text.chars().count() > MAX_MESSAGE {
        Err("too_long")
    } else {
        Ok(text.into())
    }
}

/// A name worth a lookup.
pub fn name(raw: &str) -> Option<&str> {
    let ok = !raw.is_empty()
        && raw.chars().count() <= MAX_NAME
        && !raw.chars().any(|c| c.is_control() || c.is_whitespace());
    ok.then_some(raw)
}

struct Entry {
    partner: Id,
    /// When this player last wrote a private message himself.
    wrote_at: Option<u64>,
}

/// Who each player's `/r` answers: the player he last wrote to; a player who did not write for
/// [`REPLY_WINDOW_MS`] (or never) answers whoever wrote to him last.
#[derive(Default)]
pub struct Replies(BTreeMap<Id, Entry>);

impl Replies {
    fn room(&self, id: Id) -> bool {
        self.0.contains_key(&id) || self.0.len() < MAX_REPLIES
    }

    /// `from` wrote to `to`; nothing changes for `to` (the message was quietly dropped).
    pub fn wrote(&mut self, from: Id, to: Id, now: u64) {
        if self.room(from) {
            self.0.insert(
                from,
                Entry {
                    partner: to,
                    wrote_at: Some(now),
                },
            );
        }
    }

    /// `from` wrote to `to` and `to` read it.
    pub fn delivered(&mut self, from: Id, to: Id, now: u64) {
        self.wrote(from, to, now);
        let wrote_at = self.0.get(&to).and_then(|entry| entry.wrote_at);
        let free = wrote_at.is_none_or(|at| now.saturating_sub(at) >= REPLY_WINDOW_MS);
        if free && self.room(to) {
            self.0.insert(
                to,
                Entry {
                    partner: from,
                    wrote_at,
                },
            );
        }
    }

    pub fn partner(&self, id: Id) -> Option<Id> {
        self.0.get(&id).map(|entry| entry.partner)
    }

    /// The player left.
    pub fn forget(&mut self, id: Id) {
        self.0.remove(&id);
    }

    pub fn clear(&mut self) {
        self.0.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ANN: Id = (1, 1);
    const BOB: Id = (2, 2);
    const CY: Id = (3, 3);

    #[test]
    fn a_message_is_plain_trimmed_text_of_a_chat_lines_length() {
        assert_eq!(message("  hi there  ").as_deref(), Ok("hi there"));
        assert_eq!(message("a\u{a7}cb\u{7}c\n").as_deref(), Ok("acbc"));
        assert_eq!(message("&c{0} <red>").as_deref(), Ok("&c{0} <red>"));
        assert_eq!(message(""), Err("empty"));
        assert_eq!(message(" \u{a7} \t"), Err("empty"));
        assert!(message(&"ş".repeat(MAX_MESSAGE)).is_ok());
        assert_eq!(message(&"ş".repeat(MAX_MESSAGE + 1)), Err("too_long"));
    }

    #[test]
    fn a_name_is_one_short_word() {
        assert_eq!(name("Bob_9"), Some("Bob_9"));
        assert_eq!(name(".Bedrock"), Some(".Bedrock"));
        assert_eq!(name(""), None);
        assert_eq!(name("two words"), None);
        assert_eq!(name("a\nb"), None);
        assert_eq!(name(&"x".repeat(MAX_NAME + 1)), None);
    }

    #[test]
    fn reply_answers_the_last_partner_and_a_new_writer_only_after_the_window() {
        let mut replies = Replies::default();
        assert_eq!(replies.partner(ANN), None);
        replies.delivered(ANN, BOB, 0);
        assert_eq!(replies.partner(ANN), Some(BOB));
        assert_eq!(replies.partner(BOB), Some(ANN));
        // Bob never wrote: the next writer takes his reply.
        replies.delivered(CY, BOB, 1_000);
        assert_eq!(replies.partner(BOB), Some(CY));
        // Bob answers Cy; Ann's next message does not pull his reply away while he talks to Cy.
        replies.delivered(BOB, CY, 2_000);
        replies.delivered(ANN, BOB, 3_000);
        assert_eq!(replies.partner(BOB), Some(CY));
        replies.delivered(ANN, BOB, 2_000 + REPLY_WINDOW_MS - 1);
        assert_eq!(replies.partner(BOB), Some(CY));
        replies.delivered(ANN, BOB, 2_000 + REPLY_WINDOW_MS);
        assert_eq!(replies.partner(BOB), Some(ANN));
    }

    #[test]
    fn a_dropped_message_changes_nothing_for_its_reader() {
        let mut replies = Replies::default();
        replies.wrote(ANN, BOB, 0);
        assert_eq!(replies.partner(ANN), Some(BOB));
        assert_eq!(
            replies.partner(BOB),
            None,
            "Bob cannot answer what he never read"
        );
    }

    #[test]
    fn a_player_who_left_is_forgotten_and_the_memory_is_bounded() {
        let mut replies = Replies::default();
        replies.delivered(ANN, BOB, 0);
        replies.forget(ANN);
        assert_eq!(replies.partner(ANN), None);
        assert_eq!(
            replies.partner(BOB),
            Some(ANN),
            "Bob is told Ann is gone when he answers"
        );
        replies.clear();
        for n in 0..MAX_REPLIES as u64 {
            replies.wrote((9, n), ANN, 0);
        }
        replies.delivered(BOB, CY, 0);
        assert_eq!(replies.partner(BOB), None);
        assert_eq!(replies.partner(CY), None);
        replies.forget((9, 0));
        replies.delivered(BOB, CY, 0);
        assert_eq!(replies.partner(BOB), Some(CY));
    }
}
