//! WHAT MAY LEAVE THE MACHINE — the only way to turn a capture into something an encoder accepts.
//!
//! A frame that leaves the machine can never be un-sent, so this stage fails CLOSED: a detector that
//! errors, an OCR pass that did not finish, or a frontmost app on the withhold list all produce no
//! frame at all, and the planner is told it cannot see rather than shown something unchecked.
//!
//! The order is fixed: mask at native resolution, where the accessibility tree's rectangles are
//! true, then downscale, then fingerprint. The fingerprint is of what the planner will see, which is
//! exactly what a lease names.
use serde::{Deserialize, Serialize};

use crate::frame::{fingerprint, Capture, Frame, Rect};

/// Finds rectangles that must not leave the machine.
pub trait Detector {
    fn name(&self) -> &'static str;
    /// `Err` means "could not tell", and withholds the frame. A detector never guesses empty.
    fn detect(&self, capture: &Capture) -> Result<Vec<Rect>, String>;
}

/// Password fields, from the accessibility tree. Needs no OCR and no model: the platform already
/// knows which boxes are secret, and asking it is the one detector with no false negatives on the
/// fields that matter most.
pub struct SecureFields;

impl Detector for SecureFields {
    fn name(&self) -> &'static str {
        "secure fields"
    }
    fn detect(&self, c: &Capture) -> Result<Vec<Rect>, String> {
        Ok(c.elements.iter().filter(|e| e.role == "secure_text_field").map(|e| e.rect).collect())
    }
}

/// Card numbers in OCR'd text: 13–19 digits, spaces or dashes allowed between them, that pass Luhn.
/// The Luhn check is what keeps order numbers and phone numbers visible; one in ten random digit
/// strings passes it, which is an acceptable rate for hiding things.
pub struct CardNumbers;

impl Detector for CardNumbers {
    fn name(&self) -> &'static str {
        "card numbers"
    }
    fn detect(&self, c: &Capture) -> Result<Vec<Rect>, String> {
        let spans = c.text.as_ref().ok_or("on-device text recognition did not finish")?;
        Ok(spans.iter().filter(|s| contains_card_number(&s.text)).map(|s| s.rect).collect())
    }
}

fn contains_card_number(text: &str) -> bool {
    // Split into runs of digits joined by single separators; a run ends at anything else.
    let mut digits: Vec<u8> = Vec::new();
    let mut runs = Vec::new();
    let mut last_sep = false;
    for ch in text.chars() {
        if let Some(d) = ch.to_digit(10) {
            digits.push(d as u8);
            last_sep = false;
        } else if (ch == ' ' || ch == '-') && !digits.is_empty() && !last_sep {
            last_sep = true;
        } else {
            runs.push(std::mem::take(&mut digits));
            last_sep = false;
        }
    }
    runs.push(digits);
    runs.iter().any(|r| (13..=19).contains(&r.len()) && luhn(r))
}

fn luhn(digits: &[u8]) -> bool {
    let sum: u32 = digits
        .iter()
        .rev()
        .enumerate()
        .map(|(i, &d)| {
            let d = d as u32;
            if i % 2 == 1 { if d * 2 > 9 { d * 2 - 9 } else { d * 2 } } else { d }
        })
        .sum();
    sum.is_multiple_of(10)
}

/// Why no frame was released. Said to the planner as "cannot see", and to the operator in the card.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "withheld", rename_all = "snake_case")]
pub enum Withheld {
    /// The frontmost app is one Orbit never shows to a model: a password manager, a terminal.
    App { app: String },
    /// A detector could not finish, so nothing can be said about what the frame contains.
    Detector { detector: String, why: String },
}

impl Withheld {
    pub fn narrate(&self) -> String {
        match self {
            Withheld::App { app } => format!("Not looking: {app} stays private"),
            Withheld::Detector { why, .. } => format!("Not looking: {why}"),
        }
    }
}

/// A frame that has been through every detector, masked, and downscaled. Only [`Redactor::release`]
/// makes one, so an encoder that takes `&Released` cannot be handed raw pixels by mistake.
#[derive(Debug)]
pub struct Released {
    frame: Frame,
    fingerprint: u64,
    masked: usize,
}

impl Released {
    /// What the lease for any action planned against this frame must name.
    pub fn fingerprint(&self) -> u64 {
        self.fingerprint
    }
    pub fn display(&self) -> u8 {
        self.frame.display
    }
    pub fn size(&self) -> (u32, u32) {
        (self.frame.width, self.frame.height)
    }
    /// How many regions were blacked out — counted for the zero-leakage gate, never their contents.
    pub fn masked(&self) -> usize {
        self.masked
    }
    /// The pixels, for the encoder.
    pub fn rgba(&self) -> &[u8] {
        &self.frame.rgba
    }
    pub fn pixel(&self, x: u32, y: u32) -> [u8; 4] {
        self.frame.pixel(x, y)
    }
}

pub struct Redactor {
    detectors: Vec<Box<dyn Detector + Send + Sync>>,
    /// Matched case-insensitively against the frontmost app. Default-deny for the places a secret
    /// is the whole screen (BLUEPRINT §7).
    withhold_apps: Vec<String>,
    pad: u32,
    max: (u32, u32),
}

impl Default for Redactor {
    fn default() -> Self {
        Self {
            detectors: vec![Box::new(SecureFields), Box::new(CardNumbers)],
            withhold_apps: [
                "com.1password.1password",
                "com.bitwarden.desktop",
                "com.apple.keychainaccess",
                "com.apple.Passwords",
                "1Password.exe",
                "Bitwarden.exe",
                "KeePassXC.exe",
            ]
            .map(String::from)
            .to_vec(),
            pad: 4,
            max: (1280, 800),
        }
    }
}

impl Redactor {
    pub fn new(detectors: Vec<Box<dyn Detector + Send + Sync>>, withhold_apps: Vec<String>) -> Self {
        Self { detectors, withhold_apps, ..Self::default() }
    }

    /// Capture in, frame out — or a reason there is no frame. Consumes the capture, so the unmasked
    /// buffer is gone once this returns, whichever way it goes.
    pub fn release(&self, capture: Capture) -> Result<Released, Withheld> {
        if self.withhold_apps.iter().any(|a| a.eq_ignore_ascii_case(&capture.app)) {
            return Err(Withheld::App { app: capture.app });
        }
        let mut regions = Vec::new();
        for d in &self.detectors {
            match d.detect(&capture) {
                Ok(found) => regions.extend(found),
                Err(why) => return Err(Withheld::Detector { detector: d.name().into(), why }),
            }
        }
        let mut frame = capture.frame;
        for r in &regions {
            frame.fill_black(r.padded(self.pad, frame.width, frame.height));
        }
        let frame = frame.fit(self.max.0, self.max.1);
        Ok(Released { fingerprint: fingerprint(&frame), masked: regions.len(), frame })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frame::{solid, Element, TextSpan};

    fn capture(app: &str, elements: Vec<Element>, text: Option<Vec<TextSpan>>) -> Capture {
        Capture { frame: solid(200, 100, [250, 250, 250, 255]), app: app.into(), elements, text }
    }

    #[test]
    fn a_password_field_leaves_the_machine_black() {
        let field = Element { role: "secure_text_field".into(), rect: Rect::new(20, 40, 100, 20) };
        let r = Redactor::default().release(capture("com.apple.Safari", vec![field], Some(vec![]))).unwrap();
        assert_eq!(r.masked(), 1);
        assert_eq!(r.pixel(70, 50), [0, 0, 0, 255], "inside the field");
        assert_eq!(r.pixel(17, 37), [0, 0, 0, 255], "inside the padding: focus rings bleed");
        assert_eq!(r.pixel(150, 90), [250, 250, 250, 255], "the rest of the screen is untouched");
    }

    #[test]
    fn a_card_number_in_a_pdf_is_masked_and_an_order_number_is_not() {
        let card = TextSpan { text: "Card: 4111 1111 1111 1111".into(), rect: Rect::new(0, 0, 50, 10) };
        let order = TextSpan { text: "Order 4111111111111112".into(), rect: Rect::new(0, 50, 50, 10) };
        let r = Redactor::default()
            .release(capture("com.adobe.Reader", vec![], Some(vec![card, order])))
            .unwrap();
        assert_eq!(r.masked(), 1);
        assert_eq!(r.pixel(10, 5), [0, 0, 0, 255]);
        assert_eq!(r.pixel(10, 55), [250, 250, 250, 255]);
    }

    #[test]
    fn ocr_that_did_not_finish_withholds_the_frame_rather_than_assuming_it_is_clean() {
        let out = Redactor::default().release(capture("com.apple.Safari", vec![], None));
        assert!(matches!(out, Err(Withheld::Detector { .. })));
        assert_eq!(
            out.unwrap_err().narrate(),
            "Not looking: on-device text recognition did not finish"
        );
    }

    #[test]
    fn a_password_manager_is_never_shown_to_a_model_at_all() {
        let out = Redactor::default().release(capture("com.1Password.1password", vec![], Some(vec![])));
        assert_eq!(out.unwrap_err(), Withheld::App { app: "com.1Password.1password".into() });
    }

    #[test]
    fn the_fingerprint_is_of_what_the_planner_sees() {
        // Two captures that differ only inside a password field release the same frame, so typing
        // into the field does not invalidate a plan that was made while it was empty.
        let field = Element { role: "secure_text_field".into(), rect: Rect::new(20, 40, 100, 20) };
        let a = capture("app", vec![field.clone()], Some(vec![]));
        let mut b = capture("app", vec![field], Some(vec![]));
        b.frame.rgba[(45 * 200 + 30) * 4] = 7; // a character appears in the field
        let r = Redactor::default();
        assert_eq!(r.release(a).unwrap().fingerprint(), r.release(b).unwrap().fingerprint());
    }

    #[test]
    fn luhn_accepts_test_cards_and_rejects_their_neighbours() {
        assert!(contains_card_number("5500-0000-0000-0004"));
        assert!(!contains_card_number("5500-0000-0000-0005"));
        assert!(!contains_card_number("call 0800 123 4567"));
    }
}
