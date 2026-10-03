//! The frontmost app's accessibility tree, flattened into what redaction and the lease need.
//!
//! Read in full or not at all. A walk that runs out of time, or an app that stops answering halfway,
//! yields `None` rather than the part that was read — the part that was not read may be the
//! password field, and the redactor treats "no tree" as a reason to withhold the frame.
use std::time::{Duration, Instant};

use orbit_daemon::{Element, Rect};

use crate::screen::Shot;
use crate::sys::*;

/// `kAXErrorCannotComplete`: the app did not answer in time, or at all.
const CANNOT_COMPLETE: i32 = -25204;

/// How much tree one capture may read. Chromium and Electron apps expose tens of thousands of nodes
/// for a long page; past this, the frame is withheld rather than redacted against half a tree.
pub struct Budget {
    pub nodes: usize,
    pub time: Duration,
    pub depth: usize,
}

impl Default for Budget {
    fn default() -> Self {
        Self { nodes: 6_000, time: Duration::from_millis(800), depth: 64 }
    }
}

pub struct Tree {
    pub app: String,
    pub pid: i32,
    /// `None` when the walk could not finish.
    pub elements: Option<Vec<Element>>,
    /// Why not, when it could not.
    pub incomplete: Option<String>,
    /// Nodes read, and subtrees skipped as invisible — for tuning the budget against real apps.
    pub visited: usize,
    pub pruned: usize,
}

/// Where two rectangles overlap, if they do.
fn overlap(a: Rect, b: Rect) -> Option<Rect> {
    let (x0, y0) = (a.x.max(b.x), a.y.max(b.y));
    let (x1, y1) = ((a.x + a.w).min(b.x + b.w), (a.y + a.h).min(b.y + b.h));
    (x1 > x0 && y1 > y0).then(|| Rect::new(x0, y0, x1 - x0, y1 - y0))
}

/// The attributes read for every node, fetched in ONE round trip per node. Asked one at a time they
/// cost ten IPCs each, and a modern Electron window has thousands of nodes: the difference between
/// a tree read in a few hundred milliseconds and one that never finishes.
///
/// AXValue is not among them. For text it is whatever was typed — and for an editor, the whole open
/// file — so it is read separately, only for the controls whose value is a state.
const ATTRS: [&str; 9] = [
    "AXRole", "AXSubrole", "AXTitle", "AXDescription", "AXPosition", "AXSize", "AXExpanded", "AXSelected", "AXChildren",
];
const ROLE: isize = 0;
const SUBROLE: isize = 1;
const TITLE: isize = 2;
const DESCRIPTION: isize = 3;
const POSITION: isize = 4;
const SIZE: isize = 5;
const EXPANDED: isize = 6;
const SELECTED: isize = 7;
const CHILDREN: isize = 8;

/// Copy one attribute. `Err` only for "the app is not answering"; an attribute the element does not
/// have is `Ok(None)`, which is most of them.
fn copy(el: CFTypeRef, attr: &Cf) -> Result<Option<Cf>, ()> {
    let mut out: CFTypeRef = std::ptr::null();
    // SAFETY: `el` is a live AXUIElement; `out` receives a +1 reference on success.
    let err = unsafe { AXUIElementCopyAttributeValue(el, attr.ptr(), &mut out) };
    match err {
        0 => Ok(unsafe { Cf::own(out) }),
        CANNOT_COMPLETE => Err(()),
        _ => Ok(None),
    }
}

/// Platform roles to the daemon's. Only the ones the rules refer to need to be exact; the rest are
/// kept, lowercased, because the lease compares them and a person may read them in a log.
fn role(role: &str, subrole: Option<&str>) -> String {
    match (role, subrole) {
        (_, Some("AXSecureTextField")) => "secure_text_field".into(),
        ("AXTextField" | "AXTextArea" | "AXComboBox", _) | (_, Some("AXSearchField")) => "text_field".into(),
        ("AXButton" | "AXPopUpButton" | "AXMenuButton", _) => "button".into(),
        ("AXCheckBox", Some("AXSwitch")) => "switch".into(),
        ("AXCheckBox", _) => "checkbox".into(),
        ("AXRadioButton", _) => "radio".into(),
        ("AXLink", _) => "link".into(),
        ("AXWindow", _) => "window".into(),
        (other, _) => other.trim_start_matches("AX").to_lowercase(),
    }
}

/// One node's batch of attributes. Values are borrowed from the array, which this holds.
struct Batch(Cf);

impl Batch {
    fn fetch(el: CFTypeRef, attrs: &Cf) -> Result<Batch, ()> {
        let mut out: CFTypeRef = std::ptr::null();
        // SAFETY: a live element and attribute array; `out` is a +1 CFArray on success.
        let err = unsafe { AXUIElementCopyMultipleAttributeValues(el, attrs.ptr(), 0, &mut out) };
        match (err, unsafe { Cf::own(out) }) {
            (0, Some(values)) => {
                let b = Batch(values);
                // An attribute that failed comes back as an AXValue carrying its error.
                if (0..ATTRS.len() as isize).any(|i| b.error(i) == Some(CANNOT_COMPLETE)) {
                    return Err(());
                }
                Ok(b)
            }
            _ => Err(()),
        }
    }

    fn raw(&self, i: isize) -> CFTypeRef {
        // SAFETY: the array has one entry per attribute asked for.
        unsafe { CFArrayGetValueAtIndex(self.0.ptr(), i) }
    }

    fn error(&self, i: isize) -> Option<i32> {
        let v = self.raw(i);
        // SAFETY: type-checked before reading.
        unsafe {
            if v.is_null() || CFGetTypeID(v) != AXValueGetTypeID() || AXValueGetType(v) != kAXValueAXErrorType {
                return None;
            }
            let mut code: i32 = 0;
            AXValueGetValue(v, kAXValueAXErrorType, (&mut code as *mut i32).cast());
            Some(code)
        }
    }

    /// The value, or null when the element lacks the attribute.
    fn get(&self, i: isize) -> CFTypeRef {
        if self.error(i).is_some() { std::ptr::null() } else { self.raw(i) }
    }

    fn text(&self, i: isize) -> Option<String> {
        // SAFETY: `string` type-checks.
        unsafe { string(self.get(i)) }.filter(|t| !t.is_empty())
    }

    fn flag(&self, i: isize, yes: &str, no: &str) -> Option<String> {
        let v = self.get(i);
        // SAFETY: type-checked before reading.
        unsafe {
            (!v.is_null() && CFGetTypeID(v) == CFBooleanGetTypeID())
                .then(|| if CFBooleanGetValue(v) != 0 { yes.to_string() } else { no.to_string() })
        }
    }
}

struct Walk<'a> {
    attrs: Cf,
    value: Cf,
    shot: &'a Shot,
    focused: Option<Cf>,
    budget: Budget,
    started: Instant,
    out: Vec<Element>,
    visited: usize,
    pruned: usize,
}

impl Walk<'_> {
    /// `clip` is what is visible of the nearest window or scroll area enclosing `el`, in pixels.
    /// Windows and scroll areas clip what they contain, so an element wholly outside its clip draws
    /// nothing — not it, and not its children. Skipping it hides nothing from the redactor, and is
    /// what makes a long document's tree readable at all.
    fn node(&mut self, el: CFTypeRef, depth: usize, clip: Rect) -> Result<(), String> {
        self.visited += 1;
        if self.visited > self.budget.nodes {
            return Err(format!("the tree has more than {} elements", self.budget.nodes));
        }
        if self.started.elapsed() > self.budget.time {
            return Err(format!("reading the tree took longer than {}ms", self.budget.time.as_millis()));
        }
        if depth > self.budget.depth {
            return Err(format!("the tree is deeper than {}", self.budget.depth));
        }
        let b = Batch::fetch(el, &self.attrs).map_err(|_| "the app stopped answering".to_string())?;
        let role = role(&b.text(ROLE).unwrap_or_default(), b.text(SUBROLE).as_deref());
        let label = b.text(TITLE).or_else(|| b.text(DESCRIPTION)).unwrap_or_default();

        let (p, s) = (b.get(POSITION), b.get(SIZE));
        let bounds = (!p.is_null() && !s.is_null())
            .then(|| unsafe {
                // SAFETY: AXValueGetValue checks the value's type and returns false on a mismatch.
                let mut origin = CGPoint::default();
                let mut extent = CGSize::default();
                let ok = AXValueGetValue(p, kAXValueCGPointType, (&mut origin as *mut CGPoint).cast()) != 0
                    && AXValueGetValue(s, kAXValueCGSizeType, (&mut extent as *mut CGSize).cast()) != 0;
                ok.then_some(CGRect { origin, size: extent })
            })
            .flatten();
        // Only something with real extent can be judged invisible. A zero-sized group is a
        // container that reports no frame, and its children may well be on screen.
        let sized = bounds.is_some_and(|r| r.size.width > 0.0 && r.size.height > 0.0);
        let visible = bounds.and_then(|r| self.shot.to_pixels(r)).and_then(|r| overlap(r, clip));
        if sized && visible.is_none() {
            self.pruned += 1;
            return Ok(());
        }
        let rect = bounds.and_then(|r| self.shot.to_pixels(r));
        let clip = match (role.as_str(), visible) {
            ("window" | "scrollarea", Some(v)) => v,
            _ => clip,
        };

        let state = match role.as_str() {
            "checkbox" | "switch" | "radio" => copy(el, &self.value)
                .map_err(|_| "the app stopped answering".to_string())?
                .and_then(|v| unsafe {
                    // SAFETY: an owned value, type-checked before reading.
                    let mut x: i64 = 0;
                    (CFGetTypeID(v.ptr()) == CFNumberGetTypeID()
                        && CFNumberGetValue(v.ptr(), kCFNumberSInt64Type, (&mut x as *mut i64).cast()) != 0)
                        .then(|| match x {
                            0 => "unchecked".to_string(),
                            1 => "checked".to_string(),
                            _ => "mixed".to_string(),
                        })
                }),
            _ => b.flag(EXPANDED, "expanded", "collapsed").or_else(|| b.flag(SELECTED, "selected", "unselected")),
        };

        // SAFETY: both live AXUIElements.
        let focused = self.focused.as_ref().is_some_and(|f| unsafe { CFEqual(f.ptr(), el) } != 0);
        if let Some(rect) = rect {
            self.out.push(Element { role, label, state, focused, rect });
        }

        // Children after the parent: pre-order, which is paint order.
        let kids = b.get(CHILDREN);
        // SAFETY: borrowed from the batch, which outlives the loop.
        unsafe {
            if !kids.is_null() && CFGetTypeID(kids) == CFArrayGetTypeID() {
                for i in 0..CFArrayGetCount(kids) {
                    self.node(CFArrayGetValueAtIndex(kids, i), depth + 1, clip)?;
                }
            }
        }
        Ok(())
    }
}

#[link(name = "AppKit", kind = "framework")]
unsafe extern "C" {}

/// The frontmost app's process, from NSWorkspace. A long-running daemon must spin a run loop for
/// this to track changes; a one-shot capture reads it fresh.
fn frontmost_pid() -> Option<i32> {
    use objc2::rc::Retained;
    use objc2::runtime::AnyObject;
    use objc2::{class, msg_send};
    objc2::rc::autoreleasepool(|_| {
        // SAFETY: documented NSWorkspace / NSRunningApplication accessors.
        unsafe {
            let ws: Retained<AnyObject> = msg_send![class!(NSWorkspace), sharedWorkspace];
            let app: Option<Retained<AnyObject>> = msg_send![&*ws, frontmostApplication];
            let pid: i32 = msg_send![&*app?, processIdentifier];
            Some(pid)
        }
    })
}

/// The frontmost app and its tree.
pub fn front(shot: &Shot, budget: Budget) -> Result<Tree, String> {
    // SAFETY: the system-wide element is a Create; everything else is via `copy`.
    unsafe {
        // Not the system-wide AXFocusedApplication: on macOS 27 it fails with "cannot complete"
        // while per-app queries succeed. AppKit's answer is the documented one.
        let pid = frontmost_pid().ok_or("no app is in front")?;
        let app = Cf::own(AXUIElementCreateApplication(pid)).ok_or("accessibility is unavailable")?;
        // An app that hangs must cost a quarter second, not the default six.
        AXUIElementSetMessagingTimeout(app.ptr(), 0.25);
        let id = app_id(pid).unwrap_or_else(|| format!("pid {pid}"));

        // Electron builds its tree only when asked; this asks, and is ignored by everything else.
        AXUIElementSetAttributeValue(app.ptr(), cfstr("AXManualAccessibility").ptr(), kCFBooleanTrue);

        let focused = copy(app.ptr(), &cfstr("AXFocusedUIElement")).ok().flatten();
        let names: Vec<Cf> = ATTRS.iter().map(|a| cfstr(a)).collect();
        let ptrs: Vec<CFTypeRef> = names.iter().map(Cf::ptr).collect();
        let attrs = Cf::own(CFArrayCreate(std::ptr::null(), ptrs.as_ptr(), ptrs.len() as CFIndex, &kCFTypeArrayCallBacks))
            .ok_or("could not build the attribute list")?;
        let mut walk = Walk {
            attrs,
            value: cfstr("AXValue"),
            shot,
            focused,
            budget,
            started: Instant::now(),
            out: Vec::new(),
            visited: 0,
            pruned: 0,
        };
        let screen = Rect::new(0, 0, shot.frame.width, shot.frame.height);
        let result = match copy(app.ptr(), &cfstr("AXWindows")) {
            Err(()) => Err("the app stopped answering".to_string()),
            Ok(None) => Ok(()),
            Ok(Some(windows)) => {
                // AXWindows is front to back; walk back to front so the list is in paint order.
                let n = CFArrayGetCount(windows.ptr());
                (0..n).rev().try_for_each(|i| walk.node(CFArrayGetValueAtIndex(windows.ptr(), i), 0, screen))
            }
        };
        let (visited, pruned) = (walk.visited, walk.pruned);
        Ok(match result {
            Ok(()) => Tree { app: id, pid, elements: Some(walk.out), incomplete: None, visited, pruned },
            Err(why) => Tree { app: id, pid, elements: None, incomplete: Some(why), visited, pruned },
        })
    }
}
