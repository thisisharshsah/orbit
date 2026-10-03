//! WHAT A CAPTURE IS — pixels, plus what the platform said about them.
//!
//! A frame on its own is not enough to redact. The places a secret sits are known from two other
//! sources: the accessibility tree, which says "this rectangle is a password field" without anyone
//! reading it, and on-device OCR, which finds a card number in a PDF that has no tree at all. Both
//! arrive with the pixels and travel together, so no stage can redact against a stale tree.
use serde::{Deserialize, Serialize};

/// A rectangle in device pixels of the frame it came with.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rect {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

impl Rect {
    pub fn new(x: u32, y: u32, w: u32, h: u32) -> Self {
        Self { x, y, w, h }
    }

    /// Grown by `by` on every side and clipped to a `width`×`height` frame. Masks are padded because
    /// a field's reported bounds are the box, and anti-aliased glyphs and focus rings bleed past it.
    pub fn padded(self, by: u32, width: u32, height: u32) -> Rect {
        let x0 = self.x.saturating_sub(by);
        let y0 = self.y.saturating_sub(by);
        let x1 = self.x.saturating_add(self.w).saturating_add(by).min(width);
        let y1 = self.y.saturating_add(self.h).saturating_add(by).min(height);
        Rect { x: x0, y: y0, w: x1.saturating_sub(x0), h: y1.saturating_sub(y0) }
    }

    fn right(self) -> u32 {
        self.x + self.w
    }
    fn bottom(self) -> u32 {
        self.y + self.h
    }

    /// The parts of `self` not covered by `other`: up to four rectangles, none overlapping.
    pub fn minus(self, other: Rect) -> Vec<Rect> {
        let (x0, y0) = (self.x.max(other.x), self.y.max(other.y));
        let (x1, y1) = (self.right().min(other.right()), self.bottom().min(other.bottom()));
        if x0 >= x1 || y0 >= y1 {
            return vec![self];
        }
        [
            Rect::new(self.x, self.y, self.w, y0 - self.y),             // above
            Rect::new(self.x, y1, self.w, self.bottom() - y1),          // below
            Rect::new(self.x, y0, x0 - self.x, y1 - y0),                // left
            Rect::new(x1, y0, self.right() - x1, y1 - y0),              // right
        ]
        .into_iter()
        .filter(|r| r.w > 0 && r.h > 0)
        .collect()
    }
}

/// One node of the accessibility tree, reduced to what redaction and the lease need.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Element {
    /// Platform-neutral role. The driver maps AXSecureTextField, UIA `IsPassword`, and AT-SPI
    /// `password text` all to `"secure_text_field"`.
    pub role: String,
    /// The element's own name — AXTitle or AXDescription, UIA `Name`. Never a placeholder, which
    /// vanishes on the first keystroke, and never a text field's contents.
    pub label: String,
    /// What a click would change, where there is such a thing: "checked", "unchecked", "selected",
    /// "expanded". `None` for text, whose value changing is the point of typing into it.
    pub state: Option<String>,
    /// Holds keyboard focus. At most one element per capture.
    pub focused: bool,
    pub rect: Rect,
}

/// A window on screen, and whose it is.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Window {
    /// The owning app, named the same way as [`Capture::app`].
    pub app: String,
    pub rect: Rect,
}

/// A run of text the on-device OCR found, and where.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TextSpan {
    pub text: String,
    pub rect: Rect,
}

/// Raw pixels from the platform, before anything has been removed from them.
///
/// RGBA, 8 bits per channel, rows packed. Deliberately not `Clone`: the mask is applied to *this*
/// buffer, and a copy that skipped redaction is the bug the type exists to make awkward.
#[derive(Debug)]
pub struct Frame {
    pub width: u32,
    pub height: u32,
    pub display: u8,
    pub(crate) rgba: Vec<u8>,
}

impl Frame {
    /// `None` if the buffer is not exactly `width × height × 4` bytes — a short buffer would leave
    /// part of a mask unpainted without anyone noticing.
    pub fn new(width: u32, height: u32, display: u8, rgba: Vec<u8>) -> Option<Self> {
        if width == 0 || height == 0 || rgba.len() != width as usize * height as usize * 4 {
            return None;
        }
        Some(Self { width, height, display, rgba })
    }

    pub fn pixel(&self, x: u32, y: u32) -> [u8; 4] {
        let i = (y as usize * self.width as usize + x as usize) * 4;
        [self.rgba[i], self.rgba[i + 1], self.rgba[i + 2], self.rgba[i + 3]]
    }

    /// Paint opaque black over `r`. Black, not blur: a blur of a short secret is a lookup table.
    pub(crate) fn fill_black(&mut self, r: Rect) {
        let x1 = (r.x + r.w).min(self.width);
        let y1 = (r.y + r.h).min(self.height);
        for y in r.y.min(self.height)..y1 {
            let row = y as usize * self.width as usize * 4;
            for x in r.x.min(self.width)..x1 {
                let i = row + x as usize * 4;
                self.rgba[i..i + 4].copy_from_slice(&[0, 0, 0, 255]);
            }
        }
    }

    /// Shrunk to fit inside `max_w`×`max_h`, aspect kept, by area averaging. Never enlarged.
    ///
    /// Not an optimisation: a 1080p frame costs 2.6× the visual tokens of 1024×768, every step
    /// (BLUEPRINT §6). The planner never sees native resolution.
    pub(crate) fn fit(self, max_w: u32, max_h: u32) -> Frame {
        let scale = f64::min(max_w as f64 / self.width as f64, max_h as f64 / self.height as f64);
        if scale >= 1.0 {
            return self;
        }
        let w = ((self.width as f64 * scale).round() as u32).max(1);
        let h = ((self.height as f64 * scale).round() as u32).max(1);
        let (sx, sy) = (self.width as f64 / w as f64, self.height as f64 / h as f64);
        let mut out = vec![0u8; w as usize * h as usize * 4];
        for dy in 0..h {
            let y0 = (dy as f64 * sy).floor() as u32;
            let y1 = (((dy + 1) as f64 * sy).ceil() as u32).min(self.height).max(y0 + 1);
            for dx in 0..w {
                let x0 = (dx as f64 * sx).floor() as u32;
                let x1 = (((dx + 1) as f64 * sx).ceil() as u32).min(self.width).max(x0 + 1);
                let mut acc = [0u32; 4];
                for y in y0..y1 {
                    for x in x0..x1 {
                        let p = self.pixel(x, y);
                        for c in 0..4 {
                            acc[c] += p[c] as u32;
                        }
                    }
                }
                let n = (x1 - x0) * (y1 - y0);
                let o = (dy as usize * w as usize + dx as usize) * 4;
                for c in 0..4 {
                    out[o + c] = ((acc[c] + n / 2) / n) as u8;
                }
            }
        }
        Frame { width: w, height: h, display: self.display, rgba: out }
    }
}

/// Everything one capture produced.
#[derive(Debug)]
pub struct Capture {
    pub frame: Frame,
    /// The frontmost application, as the platform names it (bundle id, executable name).
    pub app: String,
    /// The frontmost app's accessibility tree, in PAINT ORDER, back to front: a pre-order walk, so
    /// a child follows its parent and a later sibling or window is drawn over an earlier one. The
    /// lease relies on this to know what is on top at a point.
    ///
    /// `None` when the tree could not be read in full — the app did not answer, or the walk ran out
    /// of budget. A partial tree may be missing the password field, so it is not offered as one.
    pub elements: Option<Vec<Element>>,
    /// Every window on screen, back to front. Only the frontmost app's tree is read, so this is how
    /// the redactor knows which pixels belong to apps nobody looked inside. `None` if the platform
    /// would not list them.
    pub windows: Option<Vec<Window>>,
    /// `None` when OCR did not run or did not finish. Not the same as "found no text", and the
    /// redactor treats it as a reason to withhold rather than a frame with nothing to hide.
    pub text: Option<Vec<TextSpan>>,
}

/// FNV-1a. Not cryptographic and does not need to be: it tells frames and elements apart on one
/// machine, and nobody chooses the inputs to collide.
#[derive(Clone)]
pub(crate) struct Fnv(u64);

impl Fnv {
    pub(crate) fn new() -> Self {
        Self(0xcbf2_9ce4_8422_2325)
    }
    pub(crate) fn write(&mut self, bytes: &[u8]) -> &mut Self {
        for b in bytes {
            self.0 ^= *b as u64;
            self.0 = self.0.wrapping_mul(0x0000_0100_0000_01b3);
        }
        self
    }
    /// A string, terminated, so "ab"+"c" and "a"+"bc" differ.
    pub(crate) fn text(&mut self, s: &str) -> &mut Self {
        self.write(s.as_bytes()).write(&[0xff])
    }
    pub(crate) fn finish(&self) -> u64 {
        self.0
    }
}

/// The identity a lease names: dimensions and pixels of the released frame. Content-derived, so
/// the daemon can name a frame without keeping it.
pub(crate) fn fingerprint(f: &Frame) -> u64 {
    Fnv::new()
        .write(&f.width.to_le_bytes())
        .write(&f.height.to_le_bytes())
        .write(&[f.display])
        .write(&f.rgba)
        .finish()
}

#[cfg(test)]
pub(crate) fn solid(width: u32, height: u32, rgba: [u8; 4]) -> Frame {
    let buf = rgba.iter().copied().cycle().take(width as usize * height as usize * 4).collect();
    Frame::new(width, height, 0, buf).unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_buffer_of_the_wrong_size_is_not_a_frame() {
        assert!(Frame::new(2, 2, 0, vec![0; 15]).is_none());
        assert!(Frame::new(0, 2, 0, vec![]).is_none());
        assert!(Frame::new(2, 2, 0, vec![0; 16]).is_some());
    }

    #[test]
    fn subtracting_a_window_leaves_the_frame_around_it() {
        let screen = Rect::new(0, 0, 100, 100);
        let pieces = screen.minus(Rect::new(20, 30, 50, 40));
        let area: u32 = pieces.iter().map(|r| r.w * r.h).sum();
        assert_eq!(area, 100 * 100 - 50 * 40);
        assert_eq!(Rect::new(0, 0, 10, 10).minus(Rect::new(50, 50, 5, 5)), vec![Rect::new(0, 0, 10, 10)]);
        assert!(Rect::new(10, 10, 5, 5).minus(Rect::new(0, 0, 100, 100)).is_empty());
    }

    #[test]
    fn padding_grows_the_mask_and_stops_at_the_edge() {
        assert_eq!(Rect::new(10, 10, 5, 5).padded(4, 100, 100), Rect::new(6, 6, 13, 13));
        assert_eq!(Rect::new(1, 1, 5, 5).padded(4, 8, 8), Rect::new(0, 0, 8, 8));
    }

    #[test]
    fn a_1080p_capture_reaches_the_planner_at_the_budgeted_size() {
        let f = solid(1920, 1080, [200, 100, 50, 255]).fit(1280, 800);
        assert_eq!((f.width, f.height), (1280, 720));
        assert_eq!(f.pixel(640, 360), [200, 100, 50, 255]);
        // A small frame is left alone rather than upscaled into more tokens.
        let small = solid(800, 600, [0, 0, 0, 255]).fit(1280, 800);
        assert_eq!((small.width, small.height), (800, 600));
    }

    #[test]
    fn the_fingerprint_moves_when_one_pixel_does() {
        let a = solid(4, 4, [1, 2, 3, 255]);
        let mut b = solid(4, 4, [1, 2, 3, 255]);
        assert_eq!(fingerprint(&a), fingerprint(&b));
        b.fill_black(Rect::new(3, 3, 1, 1));
        assert_ne!(fingerprint(&a), fingerprint(&b));
    }
}
