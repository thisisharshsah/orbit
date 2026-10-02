//! WHAT AN ACTION WAS PLANNED AGAINST — the cheapest defence against clicking the wrong thing.
//!
//! A planner looks at a frame and says "click 452,182". By the time that answer arrives the screen
//! may have moved: a dialog opened, a list re-sorted, a page finished loading. The coordinate is
//! still valid; the thing at the coordinate is not. This is the most common way a computer-using
//! agent does something nobody asked for, and it needs no model to prevent.
//!
//! So an action carries the identity of the frame it was planned against and a deadline. The daemon
//! re-captures, compares, and refuses if either has moved on. A refusal costs one round trip. A
//! mis-click costs whatever was under the cursor.
//!
//! # What "moved on" means
//!
//! Not "any pixel changed". A real desktop is never still: a caret blinks every half second, the
//! menu-bar clock ticks, a spinner turns. A lease that demanded an identical screen would refuse
//! nearly every action, and refuse typing most of all, because typing is when a caret blinks. The
//! question is narrower — is the thing at the action's point still the thing the planner saw — and
//! answering it needs the accessibility tree and the pixels, which only the daemon has. So this type
//! checks time and display; the daemon compares what is under the point, and reports a mismatch as
//! [`LeaseError::FrameMoved`].
use serde::{Deserialize, Serialize};

/// The frame an action was planned against, and how long that remains true.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Lease {
    /// A hash of the frame the planner saw. Cheap and content-derived — the daemon does not keep the
    /// frame itself, because holding screenshots to validate screenshots is how a redaction promise
    /// gets broken. It keeps a small summary per recent frame, and this names which one.
    pub frame: u64,
    /// Unix milliseconds after which this plan is stale regardless of the screen.
    pub expires_at_ms: i64,
    /// Which display it was planned against. A two-monitor desk makes "452,182" ambiguous, and the
    /// ambiguity is invisible until the agent clicks the wrong screen.
    pub display: u8,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "error", rename_all = "snake_case")]
pub enum LeaseError {
    /// The clock ran out. The planner is answering about a screen that is seconds old.
    Expired { by_ms: i64 },
    /// What is at the action's point is not what the planner saw there. `planned` and `now` are the
    /// daemon's identities for it — an element, or a patch of pixels where there is no tree.
    FrameMoved { planned: u64, now: u64 },
    /// The plan was for another display.
    WrongDisplay { planned: u8, now: u8 },
}

impl LeaseError {
    /// Said to the operator in the card, in words that explain rather than blame.
    pub fn narrate(&self) -> String {
        match self {
            LeaseError::Expired { by_ms } => {
                format!("Skipped: the plan was {by_ms}ms out of date")
            }
            LeaseError::FrameMoved { .. } => {
                "Skipped: the screen changed while it was deciding".to_string()
            }
            LeaseError::WrongDisplay { planned, now } => {
                format!("Skipped: planned for display {planned}, you are on {now}")
            }
        }
    }
}

impl Lease {
    pub fn new(frame: u64, display: u8, expires_at_ms: i64) -> Self {
        Self { frame, expires_at_ms, display }
    }

    /// Is this plan still in date, and for this display? `now_ms` and `display` are what the daemon
    /// sees at the instant of execution — not what it saw when it asked. Whether the screen still
    /// matches is the daemon's half of the check (see the module docs).
    pub fn check(&self, now_ms: i64, display: u8) -> Result<(), LeaseError> {
        if now_ms > self.expires_at_ms {
            return Err(LeaseError::Expired { by_ms: now_ms - self.expires_at_ms });
        }
        if display != self.display {
            return Err(LeaseError::WrongDisplay { planned: self.display, now: display });
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_plan_in_date_for_this_display_passes_the_time_check() {
        assert!(Lease::new(0xABCD, 0, 10_000).check(9_000, 0).is_ok());
    }

    #[test]
    fn a_stale_plan_is_refused_even_when_the_screen_has_not_changed() {
        // A screen that has not moved is not proof the plan is current: a modal may have opened and
        // closed again between the capture and the answer.
        let lease = Lease::new(1, 0, 10_000);
        assert_eq!(lease.check(10_400, 0), Err(LeaseError::Expired { by_ms: 400 }));
    }

    #[test]
    fn a_plan_for_the_other_monitor_is_refused_before_it_is_compared() {
        let lease = Lease::new(1, 0, 10_000);
        assert_eq!(
            lease.check(5_000, 1),
            Err(LeaseError::WrongDisplay { planned: 0, now: 1 })
        );
    }

    #[test]
    fn a_refusal_explains_itself_to_the_operator() {
        let e = LeaseError::FrameMoved { planned: 1, now: 2 };
        assert_eq!(e.narrate(), "Skipped: the screen changed while it was deciding");
        assert_eq!(
            LeaseError::Expired { by_ms: 250 }.narrate(),
            "Skipped: the plan was 250ms out of date"
        );
    }
}
