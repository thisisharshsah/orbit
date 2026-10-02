//! ORBIT DAEMON — the trust boundary, as rules with no machine attached.
//!
//! Everything that crosses between this computer and the planner crosses here, in two directions:
//!
//! - **Out:** a capture becomes a [`Released`] frame only through the [`Redactor`] — masked at
//!   native resolution, downscaled, fingerprinted — or becomes nothing, with a reason.
//! - **In:** an action reaches the driver only if the [`Guard`] says the operator has not taken over
//!   and the [`Policy`] says it is in an allowed app, on the screen it was planned for, and either
//!   reversible or confirmed by a human.
//!
//! Like `orbit-core`, it does no I/O. The platform drivers hand it pixels, accessibility rectangles,
//! OCR spans and human input events; it hands back frames, verdicts, and lists of keys to release.
//! That keeps the boundary testable on any machine, and keeps every platform behind the same rules.
pub mod frame;
pub mod guard;
pub mod redact;
pub mod validate;

pub use frame::{Capture, Element, Frame, Rect, TextSpan};
pub use guard::{Guard, Halt, Human, Input, Stop};
pub use redact::{CardNumbers, Detector, Redactor, Released, SecureFields, Withheld};
pub use validate::{Observed, Policy, Verdict};
