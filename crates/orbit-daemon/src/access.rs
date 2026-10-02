//! WHAT THE OPERATING SYSTEM MUST ALLOW — and how to ask for it without wearing the operator out.
//!
//! An agent that sees and drives a desktop needs the three permissions an OS guards most closely.
//! Asking for them is the first thing a new operator meets, so it is part of the product, not a
//! setup step: one at a time, each with its reason, in an order that makes sense.
//!
//! The rules here are the same on every platform; what differs is how a driver checks and asks.
//! macOS sets the hardest constraint, and the whole flow is shaped around it: **the system prompt is
//! shown once per app, ever.** A second request is silently ignored. After the first ask the only
//! route is a deep link into System Settings — so the flow remembers what it has already asked, and
//! never shows a button that does nothing.
//!
//! The agent does not run with a permission missing. Without input monitoring it cannot hear
//! Escape, and an agent that cannot be stopped does not start (see [`crate::guard`]).
use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Permission {
    /// Pixels. macOS "Screen & System Audio Recording"; Windows needs none for its own session.
    ScreenRecording,
    /// The accessibility tree — what makes redaction and the lease possible — and, on macOS, posting
    /// synthetic input, which is gated by the same switch.
    Accessibility,
    /// Hearing the operator's own keys and mouse, for the kill switch. macOS "Input Monitoring".
    InputMonitoring,
}

impl Permission {
    /// In the order they are asked for: seeing before reading, reading before stopping. Each step
    /// is useful on its own — a person who stops after the first sees why the second exists.
    pub const ALL: [Permission; 3] = [Permission::ScreenRecording, Permission::Accessibility, Permission::InputMonitoring];

    /// The name the operator will see in System Settings, so the instruction and the screen match.
    pub fn name(self) -> &'static str {
        match self {
            Permission::ScreenRecording => "Screen & System Audio Recording",
            Permission::Accessibility => "Accessibility",
            Permission::InputMonitoring => "Input Monitoring",
        }
    }

    /// Why, in one sentence, written for someone deciding whether to trust this.
    pub fn why(self) -> &'static str {
        match self {
            Permission::ScreenRecording => {
                "To see the screen. Password fields and card numbers are blacked out on this machine before anything leaves it."
            }
            Permission::Accessibility => {
                "To know what is on screen — which box is a password field, which button is under a click — and to move the mouse and type when you ask it to."
            }
            Permission::InputMonitoring => {
                "To notice you. Pressing Escape or moving the mouse stops Orbit instantly, and that only works if it can hear your keys."
            }
        }
    }
}

/// What a driver can report. Deliberately two-valued: macOS cannot distinguish "never asked" from
/// "asked and refused" for screen recording, so neither does this — the flow's own memory of what
/// it asked fills that gap.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Granted,
    Missing,
}

/// What the operator should be shown next.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "step", content = "permission", rename_all = "snake_case")]
pub enum Step {
    /// Everything is granted.
    Ready,
    /// Show the system prompt. Offered once per permission, because it only works once.
    Ask(Permission),
    /// Already asked; the switch is in System Settings. Send them there, named exactly.
    OpenSettings(Permission),
}

/// Platform side: check, ask, and open the right pane of settings.
pub trait Access {
    fn status(&self, p: Permission) -> Status;
    /// Show the system prompt. Returns at once; the answer arrives later, and on macOS screen
    /// recording only takes effect after the app relaunches.
    fn request(&self, p: Permission);
    /// Open the settings pane where `p` is switched on. False if it could not be opened.
    fn open_settings(&self, p: Permission) -> bool;
    /// Whether a change to `p` takes effect only after a relaunch. True for macOS screen recording.
    fn needs_relaunch(&self, p: Permission) -> bool;
}

/// The flow's memory. Persisted by the driver between launches: forgetting that a prompt was
/// already shown is how a setup screen ends up with an "Allow" button that does nothing.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Asked {
    pub asked: BTreeSet<Permission>,
}

impl Asked {
    /// The next thing to show, given what the OS reports now.
    pub fn next(&self, access: &dyn Access) -> Step {
        match Permission::ALL.into_iter().find(|&p| access.status(p) == Status::Missing) {
            None => Step::Ready,
            Some(p) if self.asked.contains(&p) => Step::OpenSettings(p),
            Some(p) => Step::Ask(p),
        }
    }

    /// Carry out `step`, recording what was asked. Returns what to show afterwards — the same
    /// permission in settings, since a prompt's answer is not known until the operator acts.
    pub fn take(&mut self, step: Step, access: &dyn Access) -> Step {
        match step {
            Step::Ready => Step::Ready,
            Step::Ask(p) => {
                access.request(p);
                self.asked.insert(p);
                Step::OpenSettings(p)
            }
            Step::OpenSettings(p) => {
                access.open_settings(p);
                Step::OpenSettings(p)
            }
        }
    }

    /// Whether the agent may act at all: every permission, no exceptions.
    pub fn ready(access: &dyn Access) -> bool {
        Permission::ALL.into_iter().all(|p| access.status(p) == Status::Granted)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    /// A fake OS that behaves like macOS: the prompt appears on the first request only.
    #[derive(Default)]
    struct Mac {
        granted: RefCell<BTreeSet<Permission>>,
        prompts_shown: RefCell<Vec<Permission>>,
        opened: RefCell<Vec<Permission>>,
    }
    impl Access for Mac {
        fn status(&self, p: Permission) -> Status {
            if self.granted.borrow().contains(&p) { Status::Granted } else { Status::Missing }
        }
        fn request(&self, p: Permission) {
            if !self.prompts_shown.borrow().contains(&p) {
                self.prompts_shown.borrow_mut().push(p);
            }
        }
        fn open_settings(&self, p: Permission) -> bool {
            self.opened.borrow_mut().push(p);
            true
        }
        fn needs_relaunch(&self, p: Permission) -> bool {
            p == Permission::ScreenRecording
        }
    }

    #[test]
    fn a_new_operator_is_asked_one_thing_at_a_time_in_order() {
        let mac = Mac::default();
        let mut asked = Asked::default();
        assert_eq!(asked.next(&mac), Step::Ask(Permission::ScreenRecording));
        asked.take(Step::Ask(Permission::ScreenRecording), &mac);
        mac.granted.borrow_mut().insert(Permission::ScreenRecording);
        assert_eq!(asked.next(&mac), Step::Ask(Permission::Accessibility));
    }

    #[test]
    fn a_prompt_that_was_already_shown_is_never_offered_again() {
        let mac = Mac::default();
        let mut asked = Asked::default();
        let after = asked.take(Step::Ask(Permission::ScreenRecording), &mac);
        // Dismissed or denied: still missing. The only working route is now System Settings.
        assert_eq!(after, Step::OpenSettings(Permission::ScreenRecording));
        assert_eq!(asked.next(&mac), Step::OpenSettings(Permission::ScreenRecording));
        asked.take(Step::OpenSettings(Permission::ScreenRecording), &mac);
        assert_eq!(*mac.opened.borrow(), vec![Permission::ScreenRecording]);
        assert_eq!(mac.prompts_shown.borrow().len(), 1);
    }

    #[test]
    fn the_memory_of_what_was_asked_survives_a_relaunch() {
        let mut asked = Asked::default();
        asked.take(Step::Ask(Permission::Accessibility), &Mac::default());
        let saved = serde_json::to_string(&asked).unwrap();
        let restored: Asked = serde_json::from_str(&saved).unwrap();
        assert_eq!(restored, asked);
    }

    #[test]
    fn the_agent_does_not_run_until_it_can_be_stopped() {
        let mac = Mac::default();
        mac.granted.borrow_mut().extend([Permission::ScreenRecording, Permission::Accessibility]);
        assert!(!Asked::ready(&mac), "without input monitoring, Escape cannot be heard");
        mac.granted.borrow_mut().insert(Permission::InputMonitoring);
        assert!(Asked::ready(&mac));
        assert_eq!(Asked::default().next(&mac), Step::Ready);
    }
}
