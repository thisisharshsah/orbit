//! WHAT THE AGENT IS ALLOWED TO DO — a closed set, and the reason the rest of the system can be
//! trusted at all.
//!
//! A planner is a language model reading a screen. Screens carry text, text carries instructions,
//! and a model cannot reliably tell an instruction from a description of one. So the client does not
//! accept a command from the planner; it accepts one of these, or nothing.
//!
//! There is no `Exec`, no `Shell`, no `Script`, and no `Raw` variant. That absence is the security
//! boundary: a prompt injected through a web page can ask for a shell, and the schema has no way to
//! carry the request. Every capability added here has to be argued for on the same terms — what can
//! an attacker who controls the screen do with it.
use serde::{Deserialize, Serialize};

/// A point in the agent's own coordinate space: 0..=1000 on both axes, origin top-left.
///
/// NORMALISED, ALWAYS, and it travels with the resolution it was derived from. Pixels alone are a
/// lie on a fleet: the same "click at 452,182" lands in different places on a 4K panel at 150%
/// scaling, a 1080p monitor, and a Retina display reporting logical points. Every vendor's planner
/// expects something different, and converting at the edge — once, in the adapter that knows the
/// vendor — is the only arrangement that does not leak that difference into the rest of the system.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Point {
    pub x: u16,
    pub y: u16,
}

impl Point {
    /// Clamped rather than rejected: a planner one pixel off the edge means the right thing, and
    /// refusing the action would spend a whole round trip re-deriving it.
    pub fn new(x: i32, y: i32) -> Self {
        Self {
            x: x.clamp(0, 1000) as u16,
            y: y.clamp(0, 1000) as u16,
        }
    }

    /// To device pixels, for a screen of this size. The caller owns the rounding because it owns
    /// the display: a half-pixel matters on a 4K panel and does not on a phone.
    pub fn to_pixels(self, width: u32, height: u32) -> (u32, u32) {
        (
            (self.x as f64 / 1000.0 * width as f64).round() as u32,
            (self.y as f64 / 1000.0 * height as f64).round() as u32,
        )
    }
}

/// A key combination, named rather than encoded, so a driver can map it per platform.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct KeyChord {
    /// "cmd", "ctrl", "alt", "shift", "meta" — lowercase, deduplicated by the constructor.
    pub modifiers: Vec<String>,
    /// A single key name: "a", "enter", "escape", "tab", "f5".
    pub key: String,
}

/// The primitives. One action, one observable effect on the machine.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum Action {
    Click {
        at: Point,
        /// What the planner believed it was clicking, in its own words. Kept for the activity log
        /// and the audit trail: "clicked at 452,182" is not something a person can review, and
        /// "clicked the Search box" is.
        target: String,
    },
    DoubleClick {
        at: Point,
        target: String,
    },
    RightClick {
        at: Point,
        target: String,
    },
    /// Press at one point, release at another. Drags are a separate variant rather than a flag,
    /// because a driver implements them differently and a half-finished drag leaves the mouse down.
    Drag {
        from: Point,
        to: Point,
        target: String,
    },
    TypeText {
        text: String,
    },
    Key {
        chord: KeyChord,
    },
    Scroll {
        at: Point,
        /// Positive is down, negative is up; in notches, not pixels, because platforms disagree
        /// about how far a notch goes and the OS is the right place to decide.
        notches: i32,
    },
    /// Do nothing, on purpose, for this long. A UI that is still painting is a real reason to wait,
    /// and a planner that cannot say "wait" will click into a half-drawn dialog instead.
    Wait {
        ms: u32,
    },
    /// Take another look. The cheapest action, and the one a planner should reach for when it is
    /// unsure rather than guessing at a coordinate.
    Screenshot,
    /// STOP AND HAND BACK. Not a failure — the honest end of an attempt the agent cannot finish,
    /// including regions of the OS it is forbidden from touching (a UAC prompt, a lock screen, a
    /// biometric sheet). The operator sees why, in these words.
    Yield {
        reason: String,
    },
}

impl Action {
    /// What this action would cost if it were wrong.
    ///
    /// Classified by CONSEQUENCE, not by the planner's confidence. A confident wrong click on
    /// "Delete all" is worse than a hesitant right one, and confidence is the planner's opinion of
    /// itself — the one input least worth trusting when deciding whether to ask a human.
    pub fn consequence(&self) -> Consequence {
        match self {
            // Reading the screen and waiting change nothing.
            Action::Screenshot | Action::Wait { .. } | Action::Yield { .. } => Consequence::None,
            // Movement and scrolling are recoverable by doing them again.
            Action::Scroll { .. } => Consequence::Reversible,
            // A click may open a menu or may send a payment; the target text is the only clue
            // available here, and the policy engine — not this enum — holds the patterns.
            Action::Click { .. } | Action::DoubleClick { .. } | Action::RightClick { .. } => {
                Consequence::Reversible
            }
            Action::Drag { .. } => Consequence::Reversible,
            // Text can be a search query or a wire transfer amount. Typed input is where an
            // injected instruction does its damage, so it starts one rung higher.
            Action::TypeText { .. } => Consequence::Consequential,
            // Chords reach the parts of an application menus hide: save-as, quit, close-without-
            // saving, and every destructive shortcut an app ever bound.
            Action::Key { .. } => Consequence::Consequential,
        }
    }

    /// One line for the operator, in the present tense, saying what is happening to their machine.
    ///
    /// This is the text the overlay card shows while the agent works, so it is written for someone
    /// watching their own cursor move without having asked for this particular step.
    pub fn narrate(&self) -> String {
        match self {
            Action::Click { target, .. } => format!("Clicking {target}"),
            Action::DoubleClick { target, .. } => format!("Double-clicking {target}"),
            Action::RightClick { target, .. } => format!("Right-clicking {target}"),
            Action::Drag { target, .. } => format!("Dragging {target}"),
            Action::TypeText { text } => {
                // NEVER THE WHOLE STRING. The narration is shown on screen and kept in history,
                // and the agent types passwords, card numbers and whatever else a form asks for.
                // Length is enough for a person to recognise their own input.
                let n = text.chars().count();
                if n <= 24 {
                    format!("Typing “{text}”")
                } else {
                    format!("Typing {n} characters")
                }
            }
            Action::Key { chord } => {
                let mut parts = chord.modifiers.clone();
                parts.push(chord.key.clone());
                format!("Pressing {}", parts.join("+"))
            }
            Action::Scroll { notches, .. } => {
                if *notches >= 0 {
                    format!("Scrolling down {notches}")
                } else {
                    format!("Scrolling up {}", -notches)
                }
            }
            Action::Wait { ms } => format!("Waiting {ms}ms for the screen to settle"),
            Action::Screenshot => "Looking at the screen".to_string(),
            Action::Yield { reason } => format!("Handing back: {reason}"),
        }
    }
}

/// How much it costs to be wrong. The policy engine turns this, plus the application and the
/// target text, into allow / confirm / refuse.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Consequence {
    /// Changes nothing on the machine.
    None,
    /// Undone by doing something else.
    Reversible,
    /// Leaves a mark: text entered, a shortcut fired, a menu committed.
    Consequential,
    /// Cannot be taken back by the agent: sending, deleting, paying, granting access.
    Irreversible,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_point_is_clamped_to_the_agents_own_space() {
        assert_eq!(Point::new(-5, 10), Point { x: 0, y: 10 });
        assert_eq!(Point::new(1200, 999), Point { x: 1000, y: 999 });
    }

    #[test]
    fn the_same_point_lands_proportionally_on_any_display() {
        let centre = Point::new(500, 500);
        assert_eq!(centre.to_pixels(1920, 1080), (960, 540));
        assert_eq!(centre.to_pixels(3840, 2160), (1920, 1080));
        // A phone in portrait: the same normalised point, a different pixel.
        assert_eq!(centre.to_pixels(1080, 2400), (540, 1200));
    }

    /// The whole reason the schema exists: there is nothing here that runs a command, so a prompt
    /// injected through a rendered page cannot ask for one. If a variant is ever added that can,
    /// this test is where the argument has to be made.
    #[test]
    fn the_schema_cannot_express_arbitrary_execution() {
        let names = ["click", "double_click", "right_click", "drag", "type_text", "key", "scroll", "wait", "screenshot", "yield"];
        for forbidden in ["exec", "shell", "script", "command", "eval", "run", "raw"] {
            assert!(!names.contains(&forbidden), "{forbidden} must never be an action");
            let json = format!("{{\"action\":\"{forbidden}\"}}");
            assert!(
                serde_json::from_str::<Action>(&json).is_err(),
                "{forbidden} must not deserialise into an Action"
            );
        }
    }

    #[test]
    fn typed_text_is_never_echoed_in_full_when_it_is_long_enough_to_be_a_secret() {
        let short = Action::TypeText { text: "tokyo weather".into() };
        assert_eq!(short.narrate(), "Typing “tokyo weather”");
        let secret = Action::TypeText { text: "correct-horse-battery-staple-9182".into() };
        let line = secret.narrate();
        assert!(!line.contains("correct-horse"), "a long string must not be shown: {line}");
        assert_eq!(line, "Typing 33 characters");
    }

    #[test]
    fn typing_and_chords_outrank_clicks_because_that_is_where_the_damage_is() {
        let click = Action::Click { at: Point::new(1, 1), target: "the Search box".into() };
        let typing = Action::TypeText { text: "x".into() };
        assert!(typing.consequence() > click.consequence());
        assert_eq!(Action::Screenshot.consequence(), Consequence::None);
    }

    #[test]
    fn narration_is_written_for_the_person_watching_their_own_cursor() {
        let a = Action::Click { at: Point::new(452, 182), target: "the Search box".into() };
        assert_eq!(a.narrate(), "Clicking the Search box");
        let y = Action::Yield { reason: "this is a system password prompt".into() };
        assert_eq!(y.narrate(), "Handing back: this is a system password prompt");
    }
}
