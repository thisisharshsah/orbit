//! ORBIT CORE — the types every other crate agrees on.
//!
//! Three things live here and nothing else: what an action is ([`action`]), what the operator is
//! told about it ([`activity`]), and what an action was planned against ([`lease`]). Drivers,
//! planners, the gateway and the overlay all speak in these and share no other vocabulary.
//!
//! Deliberately free of I/O. No sockets, no screen capture, no platform code — so it compiles and
//! tests anywhere, and so the rules encoded here (a closed action set, a settled entry that cannot
//! be rewritten, a lease that expires) are testable without a machine to drive.
pub mod action;
pub mod activity;
pub mod lease;

pub use action::{Action, Consequence, KeyChord, Point};
pub use activity::{Activity, Entry, Outcome};
pub use lease::{Lease, LeaseError};
