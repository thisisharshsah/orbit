//! WHAT THE AGENT IS DOING, AND WHAT IT DID — the record the overlay card shows.
//!
//! The dot is a dot while nothing is happening. The moment the agent acts it becomes a card, and the
//! card is this log: one line per step, in the order they happened, in the present tense while
//! running and the past tense once done.
//!
//! # Why the history is the product, not a debug view
//!
//! Something else is moving the operator's cursor. The only thing that makes that tolerable is being
//! able to see, without asking, what it just did and what it is about to do — and afterwards, what
//! it did while you were reading something else. A CUA whose actions are invisible is indistinguishable
//! from malware, and will be treated as such by the person running it and by their employer's works
//! council.
//!
//! # Why entries are immutable and appended
//!
//! An action that happened cannot un-happen because a later step failed. The log is append-only and
//! each entry carries its own outcome, so "clicked Save, then the dialog never closed" survives as two
//! facts rather than being rewritten into one summary.
use serde::{Deserialize, Serialize};

use crate::action::{Action, Consequence};

/// Where an entry is in its life. An entry is created `Running` and settles exactly once.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum Outcome {
    /// Happening now. The card shows this line in the present tense, with a pulse.
    Running,
    /// The machine accepted it. `note` carries what changed, when the agent could tell.
    Done { note: Option<String> },
    /// The agent asked and the operator said no. Not an error: the system working.
    Declined,
    /// Refused before it reached the machine — by policy, by a stale lease, or by the redactor.
    Refused { why: String },
    /// Tried and did not take effect: the click landed and nothing moved, the field stayed empty.
    Failed { why: String },
    /// Interrupted by the operator — the kill switch, Escape, a hand on the mouse.
    Interrupted,
}

impl Outcome {
    pub fn is_settled(&self) -> bool {
        !matches!(self, Outcome::Running)
    }
}

/// One line of the card.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Entry {
    /// Monotonic within a session, so the card can key rows without comparing timestamps.
    pub seq: u64,
    /// Unix milliseconds when the entry was created.
    pub at_ms: i64,
    /// The operator-facing sentence — `Action::narrate()`, already redacted of long strings.
    pub line: String,
    /// What it would have cost to be wrong, so the card can mark the serious ones.
    pub consequence: Consequence,
    pub outcome: Outcome,
}

/// The session's log, and the overlay's entire model.
///
/// Bounded on purpose: a long session is thousands of steps, the card shows a handful, and holding
/// every frame's worth of narration for a day is how an overlay comes to use a gigabyte. The cap is
/// generous enough that "what did it do while I was in a meeting" is answerable.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Activity {
    entries: Vec<Entry>,
    next_seq: u64,
    cap: usize,
    /// How many were dropped off the front, so the card can say "+214 earlier" rather than
    /// implying the session started at the oldest line it still holds.
    dropped: u64,
}

impl Default for Activity {
    fn default() -> Self {
        Self::with_cap(500)
    }
}

impl Activity {
    pub fn with_cap(cap: usize) -> Self {
        Self {
            entries: Vec::new(),
            next_seq: 1,
            cap: cap.max(1),
            dropped: 0,
        }
    }

    /// Record that an action is starting. Returns the sequence number to settle it with.
    pub fn begin(&mut self, action: &Action, at_ms: i64) -> u64 {
        let seq = self.next_seq;
        self.next_seq += 1;
        self.entries.push(Entry {
            seq,
            at_ms,
            line: action.narrate(),
            consequence: action.consequence(),
            outcome: Outcome::Running,
        });
        self.trim();
        seq
    }

    /// Settle an entry. Ignores an unknown sequence and refuses to re-settle one that is done:
    /// a late acknowledgement from a driver must not rewrite a step the operator already saw fail.
    pub fn settle(&mut self, seq: u64, outcome: Outcome) -> bool {
        match self.entries.iter_mut().find(|e| e.seq == seq) {
            Some(e) if !e.outcome.is_settled() => {
                e.outcome = outcome;
                true
            }
            _ => false,
        }
    }

    /// A line that is not an action: what the agent heard, what it is thinking about, why it stopped.
    /// Settled on arrival, because there is nothing to wait for.
    pub fn note(&mut self, line: impl Into<String>, at_ms: i64) -> u64 {
        let seq = self.next_seq;
        self.next_seq += 1;
        self.entries.push(Entry {
            seq,
            at_ms,
            line: line.into(),
            consequence: Consequence::None,
            outcome: Outcome::Done { note: None },
        });
        self.trim();
        seq
    }

    /// Notes count against the cap like any other line: a session that narrates more than it acts
    /// is still a session that must not grow without bound.
    fn trim(&mut self) {
        if self.entries.len() > self.cap {
            let over = self.entries.len() - self.cap;
            self.entries.drain(0..over);
            self.dropped += over as u64;
        }
    }

    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    /// The most recent entries, oldest first — what the card renders.
    pub fn tail(&self, n: usize) -> &[Entry] {
        let start = self.entries.len().saturating_sub(n);
        &self.entries[start..]
    }

    /// The line the card headlines: whatever is happening now, or the last thing that happened.
    pub fn current(&self) -> Option<&Entry> {
        self.entries
            .iter()
            .rev()
            .find(|e| !e.outcome.is_settled())
            .or_else(|| self.entries.last())
    }

    pub fn dropped(&self) -> u64 {
        self.dropped
    }

    /// Is anything in flight? The overlay uses this to decide dot or card, and nothing else:
    /// one predicate, so the two never disagree about whether the agent is working.
    pub fn is_working(&self) -> bool {
        self.entries.iter().any(|e| !e.outcome.is_settled())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::action::{KeyChord, Point};

    fn click(target: &str) -> Action {
        Action::Click { at: Point::new(500, 500), target: target.into() }
    }

    #[test]
    fn an_entry_settles_once_and_a_late_driver_cannot_rewrite_it() {
        let mut a = Activity::default();
        let seq = a.begin(&click("Save"), 1_000);
        assert!(a.settle(seq, Outcome::Failed { why: "the dialog stayed open".into() }));
        // The driver answers late with success. The operator already saw the failure.
        assert!(!a.settle(seq, Outcome::Done { note: None }));
        assert_eq!(
            a.entries()[0].outcome,
            Outcome::Failed { why: "the dialog stayed open".into() }
        );
    }

    #[test]
    fn the_card_headlines_what_is_happening_and_falls_back_to_what_just_did() {
        let mut a = Activity::default();
        let first = a.begin(&click("the Search box"), 1);
        a.settle(first, Outcome::Done { note: None });
        // Nothing running: the headline is the last thing that happened.
        assert_eq!(a.current().unwrap().line, "Clicking the Search box");
        assert!(!a.is_working());

        let second = a.begin(&Action::TypeText { text: "tokyo weather".into() }, 2);
        assert_eq!(a.current().unwrap().line, "Typing “tokyo weather”");
        assert!(a.is_working());
        a.settle(second, Outcome::Interrupted);
        assert!(!a.is_working());
    }

    #[test]
    fn history_is_bounded_and_says_how_much_it_dropped() {
        let mut a = Activity::with_cap(3);
        for i in 0..10 {
            let seq = a.begin(&click(&format!("row {i}")), i as i64);
            a.settle(seq, Outcome::Done { note: None });
        }
        assert_eq!(a.entries().len(), 3);
        assert_eq!(a.dropped(), 7);
        // The oldest kept line is row 7, and the card can say seven earlier steps existed.
        assert_eq!(a.entries()[0].line, "Clicking row 7");
    }

    #[test]
    fn a_declined_action_is_not_an_error_and_reads_as_the_system_working() {
        let mut a = Activity::default();
        let seq = a.begin(&Action::Key { chord: KeyChord { modifiers: vec!["cmd".into()], key: "q".into() } }, 5);
        assert_eq!(a.current().unwrap().line, "Pressing cmd+q");
        a.settle(seq, Outcome::Declined);
        assert_eq!(a.entries()[0].outcome, Outcome::Declined);
        assert!(!a.is_working());
    }

    #[test]
    fn notes_carry_what_the_agent_heard_and_need_no_settling() {
        let mut a = Activity::default();
        a.note("Heard: “find the weather in Tokyo”", 1);
        assert!(!a.is_working());
        assert_eq!(a.tail(1)[0].consequence, Consequence::None);
    }

    #[test]
    fn notes_are_bounded_too() {
        let mut a = Activity::with_cap(2);
        for i in 0..5 {
            a.note(format!("heard {i}"), i);
        }
        assert_eq!(a.entries().len(), 2);
        assert_eq!(a.dropped(), 3);
    }
}
