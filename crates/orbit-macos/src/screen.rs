//! Pixels and windows: the main display as an RGBA frame, and who owns which part of it.
use std::collections::HashMap;

use orbit_daemon::{Frame, Rect, Window};

use crate::sys::*;

/// One capture of the main display, with what is needed to put other sources onto its pixels.
pub struct Shot {
    pub frame: Frame,
    /// The same pixels as a CGImage, for Vision. Held only until the capture is released.
    pub image: Cf,
    /// The display in global points, top-left origin — the space the accessibility tree and the
    /// window list report in.
    pub bounds: CGRect,
    /// Pixels per point: 2 on a Retina panel.
    pub scale: f64,
}

impl Shot {
    pub fn capture() -> Result<Shot, String> {
        // SAFETY: CGDisplayCreateImage is a Create; the bitmap context draws into `buf`, which
        // outlives it, and is released before `buf` is moved into the frame.
        unsafe {
            let display = CGMainDisplayID();
            let bounds = CGDisplayBounds(display);
            let image = Cf::own(CGDisplayCreateImage(display))
                .ok_or("the screen could not be captured: is Screen Recording switched on?")?;
            let (w, h) = (CGImageGetWidth(image.ptr()), CGImageGetHeight(image.ptr()));
            let mut buf = vec![0u8; w * h * 4];
            {
                let space = Cf::own(CGColorSpaceCreateDeviceRGB()).ok_or("no RGB colour space")?;
                let ctx = Cf::own(CGBitmapContextCreate(buf.as_mut_ptr().cast(), w, h, 8, w * 4, space.ptr(), RGBA_BITMAP_INFO))
                    .ok_or("could not create a bitmap context")?;
                let all = CGRect { origin: CGPoint::default(), size: CGSize { width: w as f64, height: h as f64 } };
                CGContextDrawImage(ctx.ptr(), all, image.ptr());
            }
            let frame = Frame::new(w as u32, h as u32, 0, buf).ok_or("the capture had no pixels")?;
            Ok(Shot { scale: w as f64 / bounds.size.width, frame, image, bounds })
        }
    }

    /// A rectangle in global points, as device pixels of this frame — grown outward to whole
    /// pixels, so a mask never falls short by a rounding error. `None` if it is off this display.
    pub fn to_pixels(&self, r: CGRect) -> Option<Rect> {
        let (fw, fh) = (self.frame.width as f64, self.frame.height as f64);
        let x0 = ((r.origin.x - self.bounds.origin.x) * self.scale).floor().clamp(0.0, fw);
        let y0 = ((r.origin.y - self.bounds.origin.y) * self.scale).floor().clamp(0.0, fh);
        let x1 = ((r.origin.x + r.size.width - self.bounds.origin.x) * self.scale).ceil().clamp(0.0, fw);
        let y1 = ((r.origin.y + r.size.height - self.bounds.origin.y) * self.scale).ceil().clamp(0.0, fh);
        (x1 > x0 && y1 > y0).then(|| Rect::new(x0 as u32, y0 as u32, (x1 - x0) as u32, (y1 - y0) as u32))
    }

    /// Every on-screen window on this display, back to front, named by owning app.
    pub fn windows(&self) -> Option<Vec<Window>> {
        // SAFETY: the list is a Copy, held for the loop; every value read from it is a Get.
        unsafe {
            let list = Cf::own(CGWindowListCopyWindowInfo(
                kCGWindowListOptionOnScreenOnly | kCGWindowListExcludeDesktopElements,
                0,
            ))?;
            let mut names: HashMap<i32, String> = HashMap::new();
            let mut out = Vec::new();
            for i in 0..CFArrayGetCount(list.ptr()) {
                let info = CFArrayGetValueAtIndex(list.ptr(), i);
                let mut pid: i64 = 0;
                let pid_ref = CFDictionaryGetValue(info, kCGWindowOwnerPID);
                if pid_ref.is_null() || CFNumberGetValue(pid_ref, kCFNumberSInt64Type, (&mut pid as *mut i64).cast()) == 0 {
                    continue;
                }
                let mut bounds = CGRect::default();
                let b = CFDictionaryGetValue(info, kCGWindowBounds);
                if b.is_null() || !CGRectMakeWithDictionaryRepresentation(b, &mut bounds) {
                    continue;
                }
                let Some(rect) = self.to_pixels(bounds) else { continue };
                let pid = pid as i32;
                let app = names.entry(pid).or_insert_with(|| app_id(pid).unwrap_or_else(|| format!("pid {pid}")));
                out.push(Window { app: app.clone(), rect });
            }
            // The window server lists front to back; the daemon wants paint order.
            out.reverse();
            out.extend(self.chrome());
            Some(out)
        }
    }
}

impl Shot {
    /// The menu bar and the Dock, as windows on top of everything else, wherever AppKit says they
    /// take space. App windows extend beneath the menu bar, and if the window list does not carry
    /// the bar itself, a background app's rectangle would be blacked out right through the menus
    /// the agent needs. Adding it here is harmless when the list does carry it.
    ///
    /// When the menu bar auto-hides, the visible frame is the whole screen and nothing is added:
    /// the top strip really is the app's pixels then, and is masked like the rest of the app.
    fn chrome(&self) -> Vec<Window> {
        use objc2::rc::Retained;
        use objc2::runtime::AnyObject;
        use objc2::{class, msg_send};
        objc2::rc::autoreleasepool(|_| {
            // SAFETY: documented NSScreen accessors; NSRect is CGRect on 64-bit.
            let (full, visible) = unsafe {
                let screens: Retained<AnyObject> = msg_send![class!(NSScreen), screens];
                let n: usize = msg_send![&*screens, count];
                if n == 0 {
                    return Vec::new();
                }
                // Index 0 is the screen with the menu bar, and the origin of AppKit's coordinates.
                let main: Retained<AnyObject> = msg_send![&*screens, objectAtIndex: 0usize];
                let full: CGRect = msg_send![&*main, frame];
                let visible: CGRect = msg_send![&*main, visibleFrame];
                (full, visible)
            };
            // AppKit is bottom-left; the window list and this frame are top-left.
            let top = |r: CGRect| full.size.height - (r.origin.y + r.size.height);
            let strip = |x: f64, y: f64, w: f64, h: f64| {
                (w > 0.0 && h > 0.0).then_some(CGRect { origin: CGPoint { x, y }, size: CGSize { width: w, height: h } })
            };
            let (fx, fw, fh) = (full.origin.x, full.size.width, full.size.height);
            let (vx, vy, vw, vh) = (visible.origin.x, top(visible), visible.size.width, visible.size.height);
            let pieces = [
                ("WindowServer", strip(fx, 0.0, fw, vy)),                          // menu bar
                ("com.apple.dock", strip(fx, vy + vh, fw, fh - (vy + vh))),         // Dock, bottom
                ("com.apple.dock", strip(fx, vy, vx - fx, vh)),                     // Dock, left
                ("com.apple.dock", strip(vx + vw, vy, fx + fw - (vx + vw), vh)),    // Dock, right
            ];
            pieces
                .into_iter()
                .filter_map(|(app, r)| Some(Window { app: app.into(), rect: self.to_pixels(r?)? }))
                .collect()
        })
    }
}

/// Write a released frame as a PNG — the redacted, downscaled pixels the planner would see, and
/// nothing else. For a person to check the mask with their own eyes; never sent anywhere.
pub fn save_png(released: &orbit_daemon::Released, path: &std::path::Path) -> Result<(), String> {
    let (w, h) = released.size();
    let mut buf = released.rgba().to_vec();
    let path = path.to_str().ok_or("the path is not UTF-8")?;
    // SAFETY: the context draws from `buf`, which outlives it; every object is an owned Cf.
    unsafe {
        let space = Cf::own(CGColorSpaceCreateDeviceRGB()).ok_or("no RGB colour space")?;
        let ctx = Cf::own(CGBitmapContextCreate(buf.as_mut_ptr().cast(), w as usize, h as usize, 8, w as usize * 4, space.ptr(), RGBA_BITMAP_INFO))
            .ok_or("could not create a bitmap context")?;
        let image = Cf::own(CGBitmapContextCreateImage(ctx.ptr())).ok_or("could not make an image")?;
        let url = cfurl(path, true).ok_or("bad path")?;
        let dest = Cf::own(CGImageDestinationCreateWithURL(url.ptr(), cfstr("public.png").ptr(), 1, std::ptr::null()))
            .ok_or("could not open the file for writing")?;
        CGImageDestinationAddImage(dest.ptr(), image.ptr(), std::ptr::null());
        if !CGImageDestinationFinalize(dest.ptr()) {
            return Err("could not write the PNG".into());
        }
    }
    Ok(())
}
