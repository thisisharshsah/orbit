//! On-device text recognition, through Vision. Nothing leaves the machine: this is how the redactor
//! finds a card number in a PDF or a screenshot of a statement, where there is no tree to ask.
//!
//! Fast recognition, no language correction. The redactor needs digits in the right places, not
//! prose; correction "fixes" a card number into words, and the accurate model costs several times
//! the latency on a full Retina frame.
use std::ffi::{c_void, CStr};

use objc2::encode::{Encode, Encoding, RefEncode};
use objc2::rc::{autoreleasepool, Allocated, Retained};
use objc2::runtime::{AnyObject, Bool};
use objc2::{class, msg_send};
use orbit_daemon::{Rect, TextSpan};

use crate::screen::Shot;
use crate::sys::{CGPoint, CGRect, CGSize};

#[link(name = "Vision", kind = "framework")]
unsafe extern "C" {}

// SAFETY: these match the C layouts, all doubles on 64-bit.
unsafe impl Encode for CGPoint {
    const ENCODING: Encoding = Encoding::Struct("CGPoint", &[f64::ENCODING, f64::ENCODING]);
}
unsafe impl Encode for CGSize {
    const ENCODING: Encoding = Encoding::Struct("CGSize", &[f64::ENCODING, f64::ENCODING]);
}
unsafe impl Encode for CGRect {
    const ENCODING: Encoding = Encoding::Struct("CGRect", &[CGPoint::ENCODING, CGSize::ENCODING]);
}

/// A borrowed CGImageRef, typed so the message carries the right encoding.
#[repr(transparent)]
struct CGImageRef(*const c_void);
// SAFETY: a pointer to the opaque CGImage struct.
unsafe impl Encode for CGImageRef {
    const ENCODING: Encoding = Encoding::Pointer(&Encoding::Struct("CGImage", &[]));
}
unsafe impl RefEncode for CGImageRef {
    const ENCODING_REF: Encoding = Encoding::Pointer(&<Self as Encode>::ENCODING);
}

/// `VNRequestTextRecognitionLevelFast`.
const FAST: isize = 1;

/// Every line of text Vision finds in the shot, in the shot's pixels.
pub fn recognise(shot: &Shot) -> Result<Vec<TextSpan>, String> {
    let (w, h) = (shot.frame.width as f64, shot.frame.height as f64);
    autoreleasepool(|_| {
        // SAFETY: standard Vision usage; every object is a Retained held for the duration, and the
        // CGImage is borrowed from the shot, which outlives this call.
        unsafe {
            let request: Retained<AnyObject> = msg_send![class!(VNRecognizeTextRequest), new];
            let _: () = msg_send![&*request, setRecognitionLevel: FAST];
            let _: () = msg_send![&*request, setUsesLanguageCorrection: Bool::NO];

            let options: Retained<AnyObject> = msg_send![class!(NSDictionary), dictionary];
            let handler: Allocated<AnyObject> = msg_send![class!(VNImageRequestHandler), alloc];
            let handler: Option<Retained<AnyObject>> =
                msg_send![handler, initWithCGImage: CGImageRef(shot.image.ptr()), options: &*options];
            let handler = handler.ok_or("Vision would not take the image")?;

            let requests: Retained<AnyObject> = msg_send![class!(NSArray), arrayWithObject: &*request];
            let ok: Bool = msg_send![&*handler, performRequests: &*requests, error: std::ptr::null_mut::<*mut AnyObject>()];
            if !ok.as_bool() {
                return Err("on-device text recognition failed".to_string());
            }

            let mut spans = Vec::new();
            let results: Option<Retained<AnyObject>> = msg_send![&*request, results];
            let Some(results) = results else { return Ok(spans) };
            let n: usize = msg_send![&*results, count];
            for i in 0..n {
                let obs: Retained<AnyObject> = msg_send![&*results, objectAtIndex: i];
                let candidates: Retained<AnyObject> = msg_send![&*obs, topCandidates: 1usize];
                let best: Option<Retained<AnyObject>> = msg_send![&*candidates, firstObject];
                let Some(best) = best else { continue };
                let string: Retained<AnyObject> = msg_send![&*best, string];
                let utf8: *const std::ffi::c_char = msg_send![&*string, UTF8String];
                if utf8.is_null() {
                    continue;
                }
                let text = CStr::from_ptr(utf8).to_string_lossy().into_owned();
                // Normalised, origin bottom-left; the frame's origin is top-left.
                let b: CGRect = msg_send![&*obs, boundingBox];
                let x0 = (b.origin.x * w).floor().max(0.0);
                let y0 = ((1.0 - b.origin.y - b.size.height) * h).floor().max(0.0);
                let x1 = ((b.origin.x + b.size.width) * w).ceil().min(w);
                let y1 = ((1.0 - b.origin.y) * h).ceil().min(h);
                if x1 > x0 && y1 > y0 {
                    let rect = Rect::new(x0 as u32, y0 as u32, (x1 - x0) as u32, (y1 - y0) as u32);
                    spans.push(TextSpan { text, rect });
                }
            }
            Ok(spans)
        }
    })
}
