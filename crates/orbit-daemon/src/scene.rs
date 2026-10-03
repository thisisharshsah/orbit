//! WHAT IS AT THE POINT — the daemon's half of the lease.
//!
//! A lease names the frame an action was planned against. Comparing that whole frame with the
//! screen now would refuse almost everything — a caret blinks, a clock ticks — so the daemon keeps
//! a small summary of each recent frame and compares only what the action depends on:
//!
//! - **A pointer action** depends on the topmost element under its point: role, label, bounds, and
//!   any state a click would flip. A banner that slides over the button, a list that re-sorts, a
//!   checkbox someone else ticked — all change it. A caret blinking in the field beside it does not.
//!   If the topmost thing there is window-sized — a pane, or a modal's dimmed backdrop — it says
//!   nothing about the point, and the pixels decide instead. Never the element underneath it: that
//!   is exactly the thing that can no longer be clicked.
//! - **A keyboard action** depends on which element has focus, because that is where the keys go.
//!   A focused field whose bounds change (a composer growing a line) refuses once and is re-planned.
//! - **Where there is no tree** — a canvas, a game, a remote desktop — the fallback is the pixels
//!   around the point, or for keys the whole frame. Strict, and only used when nothing better exists.
//!
//! The summary is hashes and rectangles; the pixels are not kept.
use std::collections::VecDeque;

use orbit_core::{Action, Point};

use crate::frame::{Element, Fnv, Frame};

/// The fallback grid: the released frame cut into GRID×GRID tiles, each hashed.
const GRID: u32 = 32;
/// An element covering more than a quarter of the screen is a window, a pane or a backdrop, not a
/// target, and says nothing about what is under the point. Units are thousandths squared.
const CONTROL_MAX_AREA: u32 = 250_000;

#[derive(Debug, Clone)]
struct Node {
    id: u64,
    /// Bounds in the agent's 0..=1000 space, the same space as [`Point`].
    x0: u16,
    y0: u16,
    x1: u16,
    y1: u16,
}

impl Node {
    fn area(&self) -> u32 {
        (self.x1 - self.x0) as u32 * (self.y1 - self.y0) as u32
    }
    fn contains(&self, p: Point) -> bool {
        (self.x0..=self.x1).contains(&p.x) && (self.y0..=self.y1).contains(&p.y)
    }
}

#[derive(Debug, Clone)]
pub struct Scene {
    fingerprint: u64,
    nodes: Vec<Node>,
    focus: Option<u64>,
    tiles: Vec<u64>,
}

impl Scene {
    /// `elements` are in the native pixels of a `native_w`×`native_h` capture; `released` is the
    /// downscaled frame the planner sees, whose tiles are the fallback.
    pub(crate) fn new(fingerprint: u64, elements: &[Element], native_w: u32, native_h: u32, released: &Frame) -> Self {
        let norm = |v: u32, of: u32, up: bool| -> u16 {
            let n = v as u64 * 1000;
            let q = if up { n.div_ceil(of as u64) } else { n / of as u64 };
            q.min(1000) as u16
        };
        let mut focus = None;
        let nodes = elements
            .iter()
            .map(|e| {
                let mut h = Fnv::new();
                h.text(&e.role).text(&e.label).text(e.state.as_deref().unwrap_or("\u{0}"));
                for v in [e.rect.x, e.rect.y, e.rect.w, e.rect.h] {
                    h.write(&v.to_le_bytes());
                }
                let id = h.finish();
                if e.focused {
                    focus = Some(id);
                }
                Node {
                    id,
                    x0: norm(e.rect.x, native_w, false),
                    y0: norm(e.rect.y, native_h, false),
                    x1: norm(e.rect.x + e.rect.w, native_w, true),
                    y1: norm(e.rect.y + e.rect.h, native_h, true),
                }
            })
            .collect();

        let (w, h) = (released.width, released.height);
        let mut tiles = vec![Fnv::new(); (GRID * GRID) as usize];
        for y in 0..h {
            let ty = (y * GRID / h) as usize;
            for tx in 0..GRID {
                let (x0, x1) = (tx * w / GRID, (tx + 1) * w / GRID);
                let row = (y * w) as usize * 4;
                tiles[ty * GRID as usize + tx as usize].write(&released.rgba[row + x0 as usize * 4..row + x1 as usize * 4]);
            }
        }
        Self { fingerprint, nodes, focus, tiles: tiles.iter().map(Fnv::finish).collect() }
    }

    pub fn fingerprint(&self) -> u64 {
        self.fingerprint
    }

    /// What `action` depends on in this scene, as one comparable value. `None` for actions that
    /// touch nothing and so depend on nothing.
    pub fn basis(&self, action: &Action) -> Option<u64> {
        match action {
            Action::Click { at, .. }
            | Action::DoubleClick { at, .. }
            | Action::RightClick { at, .. }
            | Action::Scroll { at, .. } => Some(self.at(*at)),
            // Where it picks up and where it lets go both matter.
            Action::Drag { from, to, .. } => Some(
                Fnv::new().write(&self.at(*from).to_le_bytes()).write(&self.at(*to).to_le_bytes()).finish(),
            ),
            Action::TypeText { .. } | Action::Key { .. } => Some(match self.focus {
                Some(id) => tag(b'F', id),
                None => tag(b'S', self.fingerprint),
            }),
            Action::Screenshot | Action::Wait { .. } | Action::Yield { .. } => None,
        }
    }

    fn at(&self, p: Point) -> u64 {
        // Nodes are in paint order, so the last one containing the point is the one on top.
        if let Some(n) = self.nodes.iter().rev().find(|n| n.contains(p))
            && n.area() <= CONTROL_MAX_AREA
        {
            return tag(b'E', n.id);
        }
        let cell = |v: u16| (v as u32 * GRID / 1001) as i64;
        let (cx, cy) = (cell(p.x), cell(p.y));
        let mut h = Fnv::new();
        h.write(b"P");
        for ty in cy - 1..=cy + 1 {
            for tx in cx - 1..=cx + 1 {
                if (0..GRID as i64).contains(&tx) && (0..GRID as i64).contains(&ty) {
                    h.write(&self.tiles[(ty * GRID as i64 + tx) as usize].to_le_bytes());
                }
            }
        }
        h.finish()
    }
}

/// The kind of thing a basis names goes into its value, so an element can never compare equal to a
/// patch of pixels that happens to share a hash.
fn tag(kind: u8, id: u64) -> u64 {
    Fnv::new().write(&[kind]).write(&id.to_le_bytes()).finish()
}

/// The last few scenes released to the planner, by fingerprint. A lease naming one that has aged
/// out is refused, which is right: the plan is older than anything the daemon can still check.
pub struct Recent {
    scenes: VecDeque<Scene>,
    cap: usize,
}

impl Default for Recent {
    fn default() -> Self {
        Self { scenes: VecDeque::new(), cap: 8 }
    }
}

impl Recent {
    pub fn remember(&mut self, scene: Scene) {
        if self.get(scene.fingerprint).is_some() {
            return;
        }
        if self.scenes.len() == self.cap {
            self.scenes.pop_front();
        }
        self.scenes.push_back(scene);
    }

    pub fn get(&self, fingerprint: u64) -> Option<&Scene> {
        self.scenes.iter().find(|s| s.fingerprint == fingerprint)
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::frame::{Capture, Frame, Rect};
    use crate::redact::Redactor;

    /// A 1000×500 window, so native pixels and the agent's space line up on the x axis.
    pub(crate) fn scene_with(elements: Vec<Element>, paint: impl Fn(&mut [u8])) -> Scene {
        let mut px = vec![240u8; 1000 * 500 * 4];
        paint(&mut px);
        let capture = Capture {
            frame: Frame::new(1000, 500, 0, px).unwrap(),
            app: "app".into(),
            elements: Some(elements),
            windows: Some(vec![]),
            text: Some(vec![]),
        };
        Redactor::default().release(capture).unwrap().scene().clone()
    }

    pub(crate) fn field(label: &str, x: u32, focused: bool) -> Element {
        Element { role: "text_field".into(), label: label.into(), focused, rect: Rect::new(x, 100, 200, 30), ..Default::default() }
    }

    pub(crate) fn caret(px: &mut [u8]) {
        for y in 105..125 {
            px[(y * 1000 + 150) * 4] = 0;
        }
    }

    fn click(x: i32, y: i32) -> Action {
        Action::Click { at: Point::new(x, y), target: "it".into() }
    }

    #[test]
    fn a_blinking_caret_does_not_move_the_element_under_the_point() {
        let off = scene_with(vec![field("Search", 100, true)], |_| {});
        let on = scene_with(vec![field("Search", 100, true)], caret);
        assert_ne!(off.fingerprint(), on.fingerprint(), "the frames do differ");
        assert_eq!(off.basis(&click(200, 230)), on.basis(&click(200, 230)));
        let typing = Action::TypeText { text: "tokyo".into() };
        assert_eq!(off.basis(&typing), on.basis(&typing));
    }

    #[test]
    fn a_different_element_at_the_point_is_a_different_basis() {
        let before = scene_with(vec![field("Search", 100, false)], |_| {});
        let dialog = Element { role: "button".into(), label: "Delete".into(), rect: Rect::new(150, 90, 100, 50), ..Default::default() };
        let after = scene_with(vec![field("Search", 100, false), dialog], |_| {});
        assert_ne!(before.basis(&click(200, 230)), after.basis(&click(200, 230)));
        // A click elsewhere on the screen did not depend on the dialog.
        assert_eq!(before.basis(&click(800, 800)), after.basis(&click(800, 800)));
    }

    #[test]
    fn a_ticked_checkbox_is_not_the_checkbox_the_planner_saw() {
        let b = |state: &str| Element {
            role: "checkbox".into(),
            label: "Remember me".into(),
            state: Some(state.into()),
            rect: Rect::new(100, 300, 20, 20),
            ..Default::default()
        };
        let p = click(110, 620);
        assert_ne!(scene_with(vec![b("unchecked")], |_| {}).basis(&p), scene_with(vec![b("checked")], |_| {}).basis(&p));
    }

    #[test]
    fn focus_moving_to_another_field_changes_where_the_keys_go() {
        let typing = Action::TypeText { text: "x".into() };
        let a = scene_with(vec![field("To", 100, true), field("Subject", 400, false)], |_| {});
        let b = scene_with(vec![field("To", 100, false), field("Subject", 400, true)], |_| {});
        assert_ne!(a.basis(&typing), b.basis(&typing));
    }

    #[test]
    fn with_no_tree_the_pixels_near_the_point_decide_and_far_ones_do_not() {
        let blank = scene_with(vec![], |_| {});
        let near = scene_with(vec![], |px| px[(250 * 1000 + 505) * 4] = 0);
        let far = scene_with(vec![], |px| px[(10 * 1000 + 10) * 4] = 0);
        let p = click(500, 500);
        assert_ne!(blank.basis(&p), near.basis(&p));
        assert_eq!(blank.basis(&p), far.basis(&p));
    }

    #[test]
    fn a_banner_over_the_target_wins_even_though_the_target_is_smaller() {
        let button = Element { role: "button".into(), label: "Sign in".into(), rect: Rect::new(400, 200, 100, 30), ..Default::default() };
        let banner = Element { role: "button".into(), label: "Accept all".into(), rect: Rect::new(300, 180, 400, 100), ..Default::default() };
        let p = click(450, 430);
        let before = scene_with(vec![button.clone()], |_| {});
        assert_ne!(before.basis(&p), scene_with(vec![button.clone(), banner.clone()], |_| {}).basis(&p));
        // The same two, with the button drawn on top: nothing covers it, and the plan stands.
        assert_eq!(before.basis(&p), scene_with(vec![banner, button], |_| {}).basis(&p));
    }

    #[test]
    fn a_modal_backdrop_over_the_target_is_not_looked_through() {
        let button = Element { role: "button".into(), label: "Sign in".into(), rect: Rect::new(400, 200, 100, 30), ..Default::default() };
        let backdrop = Element { role: "group".into(), rect: Rect::new(0, 0, 1000, 500), ..Default::default() };
        let p = click(450, 430);
        let before = scene_with(vec![button.clone()], |_| {});
        let dimmed = scene_with(vec![button, backdrop], |px| px.iter_mut().for_each(|c| *c /= 2));
        assert_ne!(before.basis(&p), dimmed.basis(&p));
    }

    #[test]
    fn a_window_sized_element_is_not_mistaken_for_the_target() {
        let window = Element { role: "window".into(), rect: Rect::new(0, 0, 1000, 500), ..Default::default() };
        let blank = scene_with(vec![window.clone()], |_| {});
        let changed = scene_with(vec![window], |px| px[(250 * 1000 + 505) * 4] = 0);
        assert_ne!(blank.basis(&click(500, 500)), changed.basis(&click(500, 500)));
    }

    #[test]
    fn recent_scenes_are_bounded() {
        let mut r = Recent { scenes: VecDeque::new(), cap: 2 };
        let s: Vec<Scene> = (0..3u8).map(|i| scene_with(vec![], |px| px[0] = i)).collect();
        let fps: Vec<u64> = s.iter().map(Scene::fingerprint).collect();
        s.into_iter().for_each(|x| r.remember(x));
        assert!(r.get(fps[0]).is_none());
        assert!(r.get(fps[2]).is_some());
    }
}
