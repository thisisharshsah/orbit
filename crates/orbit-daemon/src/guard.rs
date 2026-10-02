//! WHEN TO LET GO — the kill switch, the watchdog, and the record of what is held down.
//!
//! The operator must be able to take their machine back by doing what anyone does when something
//! else is moving their cursor: grab the mouse, or hit Escape. And a dropped connection must never
//! leave Shift down. Both end the same way: every key and button the agent is holding is released,
//! and nothing more runs until the operator says so.
//!
//! The guard only ever sees HUMAN input. The driver separates it from the agent's own synthesised
//! events (the event source on macOS, `LLMHF_INJECTED` on Windows) before it gets here; otherwise the
//! agent's own drag would trip the switch it is guarded by.
use std::collections::{BTreeSet, VecDeque};

use serde::{Deserialize, Serialize};

/// Something the agent can hold down.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(tag = "input", rename_all = "snake_case")]
pub enum Input {
    Key { key: String },
    Button { button: String },
}

/// Why the agent stopped. Each one reads as the operator being in charge, not as a fault.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "stop", rename_all = "snake_case")]
pub enum Stop {
    /// A hand on the mouse.
    Mouse,
    Escape,
    Hotkey,
    /// The operator switched app or window themselves.
    Focus,
    /// The gateway went quiet for longer than the agent may hold input unsupervised.
    Silence { ms: i64 },
}

impl Stop {
    pub fn narrate(&self) -> String {
        match self {
            Stop::Mouse => "Stopped: you moved the mouse".into(),
            Stop::Escape => "Stopped: you pressed Escape".into(),
            Stop::Hotkey => "Stopped: you pressed the stop shortcut".into(),
            Stop::Focus => "Stopped: you switched windows".into(),
            Stop::Silence { ms } => format!("Stopped: lost the connection for {ms}ms"),
        }
    }
}

/// What the operator's hands did.
#[derive(Debug, Clone, PartialEq)]
pub enum Human {
    MouseMoved { x: f64, y: f64 },
    Escape,
    Hotkey,
    FocusChanged,
}

/// The daemon's half of a stop: why, and what the driver must release, in one value so the two
/// cannot be separated.
#[derive(Debug, Clone, PartialEq)]
pub struct Halt {
    pub stop: Stop,
    pub release: Vec<Input>,
}

pub struct Guard {
    held: BTreeSet<Input>,
    stopped: Option<Stop>,
    trail: VecDeque<(i64, f64, f64)>,
    last_heard_ms: i64,
    /// Physical travel within `window_ms` that counts as a hand on the mouse.
    pub travel_px: f64,
    pub window_ms: i64,
    pub silence_ms: i64,
}

impl Guard {
    pub fn new(now_ms: i64) -> Self {
        Self {
            held: BTreeSet::new(),
            stopped: None,
            trail: VecDeque::new(),
            last_heard_ms: now_ms,
            // BLUEPRINT §7: ~50px in 100ms, and 300ms of silence.
            travel_px: 50.0,
            window_ms: 100,
            silence_ms: 300,
        }
    }

    /// Is the agent allowed to act? False from the moment of any stop until [`Guard::resume`].
    pub fn may_act(&self) -> bool {
        self.stopped.is_none()
    }

    pub fn stopped(&self) -> Option<&Stop> {
        self.stopped.as_ref()
    }

    /// The driver pressed something on the agent's behalf.
    pub fn pressed(&mut self, input: Input) {
        self.held.insert(input);
    }

    /// The driver released something.
    pub fn released(&mut self, input: &Input) {
        self.held.remove(input);
    }

    /// Anything from the gateway — a plan, a heartbeat — counts as being supervised.
    pub fn heard(&mut self, now_ms: i64) {
        self.last_heard_ms = now_ms;
    }

    /// Called on the daemon's own clock, every frame. Trips on silence.
    pub fn tick(&mut self, now_ms: i64) -> Option<Halt> {
        let quiet = now_ms - self.last_heard_ms;
        if quiet > self.silence_ms {
            return self.halt(Stop::Silence { ms: quiet });
        }
        None
    }

    /// Called for every human input event.
    pub fn human(&mut self, at_ms: i64, event: Human) -> Option<Halt> {
        match event {
            Human::Escape => self.halt(Stop::Escape),
            Human::Hotkey => self.halt(Stop::Hotkey),
            Human::FocusChanged => self.halt(Stop::Focus),
            Human::MouseMoved { x, y } => {
                self.trail.push_back((at_ms, x, y));
                while self.trail.front().is_some_and(|&(t, ..)| at_ms - t > self.window_ms) {
                    self.trail.pop_front();
                }
                // Path length, not displacement: a hand shaking the mouse to make it stop goes
                // nowhere and should still count.
                let travel: f64 = self
                    .trail
                    .iter()
                    .zip(self.trail.iter().skip(1))
                    .map(|(a, b)| ((b.1 - a.1).powi(2) + (b.2 - a.2).powi(2)).sqrt())
                    .sum();
                if travel > self.travel_px { self.halt(Stop::Mouse) } else { None }
            }
        }
    }

    /// The operator handing control back. Starts the silence clock afresh.
    pub fn resume(&mut self, now_ms: i64) {
        self.stopped = None;
        self.trail.clear();
        self.last_heard_ms = now_ms;
    }

    /// Stop, and hand over everything held. Releasing is idempotent at the driver, so a second stop
    /// while already stopped still releases whatever was pressed since — but reports only once.
    fn halt(&mut self, stop: Stop) -> Option<Halt> {
        let release: Vec<Input> = std::mem::take(&mut self.held).into_iter().collect();
        if self.stopped.is_some() {
            return (!release.is_empty()).then(|| Halt { stop: self.stopped.clone().unwrap(), release });
        }
        self.stopped = Some(stop.clone());
        Some(Halt { stop, release })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shift() -> Input {
        Input::Key { key: "shift".into() }
    }

    #[test]
    fn a_dropped_connection_never_leaves_shift_down() {
        let mut g = Guard::new(0);
        g.pressed(shift());
        assert_eq!(g.tick(300), None, "300ms is the limit, not past it");
        let halt = g.tick(301).unwrap();
        assert_eq!(halt.stop, Stop::Silence { ms: 301 });
        assert_eq!(halt.release, vec![shift()]);
        assert!(!g.may_act());
    }

    #[test]
    fn a_hand_on_the_mouse_stops_it_and_a_nudge_does_not() {
        let mut g = Guard::new(0);
        assert_eq!(g.human(0, Human::MouseMoved { x: 0.0, y: 0.0 }), None);
        assert_eq!(g.human(40, Human::MouseMoved { x: 3.0, y: 4.0 }), None, "5px is a desk bump");
        // 200ms later the old samples have aged out, so slow drift never adds up to a grab.
        assert_eq!(g.human(240, Human::MouseMoved { x: 40.0, y: 4.0 }), None);
        let halt = g.human(280, Human::MouseMoved { x: 100.0, y: 4.0 }).unwrap();
        assert_eq!(halt.stop, Stop::Mouse);
        assert_eq!(halt.stop.narrate(), "Stopped: you moved the mouse");
    }

    #[test]
    fn shaking_the_mouse_in_place_counts_as_grabbing_it() {
        let mut g = Guard::new(0);
        let mut out = None;
        for (i, x) in [0.0, 20.0, 0.0, 20.0, 0.0].into_iter().enumerate() {
            out = out.or(g.human(i as i64 * 10, Human::MouseMoved { x, y: 0.0 }));
        }
        assert_eq!(out.unwrap().stop, Stop::Mouse);
    }

    #[test]
    fn escape_releases_everything_held_and_nothing_runs_until_resumed() {
        let mut g = Guard::new(0);
        g.pressed(shift());
        g.pressed(Input::Button { button: "left".into() });
        g.released(&shift());
        let halt = g.human(10, Human::Escape).unwrap();
        assert_eq!(halt.release, vec![Input::Button { button: "left".into() }]);
        // Already stopped, nothing held: nothing more to say.
        assert_eq!(g.human(20, Human::Escape), None);
        g.resume(1_000);
        assert!(g.may_act());
        assert_eq!(g.tick(1_200), None, "the silence clock restarts on resume");
    }
}
