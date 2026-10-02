//! WHAT A CAPTURE IS — pixels, plus what the platform said about them.
//!
//! A frame on its own is not enough to redact. The places a secret sits are known from two other
//! sources: the accessibility tree, which says "this rectangle is a password field" without anyone
//! reading it, and on-device OCR, which finds a card number in a PDF that has no tree at all. Both
//! arrive with the pixels and travel together, so no stage can redact against a stale tree.
use serde::{Deserialize, Serialize};

/// A rectangle in device pixels of the frame it came with.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
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
}

/// One node of the accessibility tree, reduced to what redaction needs.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Element {
    /// Platform-neutral role. The driver maps AXSecureTextField, UIA `IsPassword`, and AT-SPI
    /// `password text` all to `"secure_text_field"`.
    pub role: String,
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
    pub elements: Vec<Element>,
    /// `None` when OCR did not run or did not finish. Not the same as "found no text", and the
    /// redactor treats it as a reason to withhold rather than a frame with nothing to hide.
    pub text: Option<Vec<TextSpan>>,
}

/// FNV-1a over dimensions and pixels: the identity a lease names. Content-derived, so the daemon
/// can validate a lease without keeping the frame the planner saw.
pub(crate) fn fingerprint(f: &Frame) -> u64 {
    const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01b3;
    let mut h = OFFSET;
    let header = [f.width.to_le_bytes(), f.height.to_le_bytes()];
    for b in header.iter().flatten().chain([f.display].iter()).chain(f.rgba.iter()) {
        h ^= *b as u64;
        h = h.wrapping_mul(PRIME);
    }
    h
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
