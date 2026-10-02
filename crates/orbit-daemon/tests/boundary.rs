//! One step of the loop, across the boundary in both directions: a frame out, a plan back, an
//! action checked against the screen as it is now, and the operator's card told what happened.
use orbit_core::{Action, Activity, Lease, Outcome, Point};
use orbit_daemon::{Capture, Element, Frame, Guard, Human, Observed, Policy, Recent, Rect, Redactor, Verdict};

/// A sign-in page at 1080p: an email field, a password field, a Sign in button. `caret` blinks in
/// the focused email field; `typed` changes a pixel inside the password field.
fn login_page(caret: bool, typed: u8, extra: Option<Element>) -> Capture {
    let mut px = vec![240u8; 1920 * 1080 * 4];
    px[(500 * 1920 + 900) * 4] = typed;
    if caret {
        for y in 410..430 {
            px[(y * 1920 + 820) * 4] = 0;
        }
    }
    let mut elements = vec![
        Element { role: "text_field".into(), label: "Email".into(), focused: true, rect: Rect::new(800, 400, 320, 40), ..Default::default() },
        Element { role: "secure_text_field".into(), label: "Password".into(), rect: Rect::new(800, 480, 320, 40), ..Default::default() },
        Element { role: "button".into(), label: "Sign in".into(), rect: Rect::new(800, 560, 320, 48), ..Default::default() },
    ];
    elements.extend(extra);
    Capture { frame: Frame::new(1920, 1080, 0, px).unwrap(), app: "com.apple.Safari".into(), elements, text: Some(vec![]) }
}

fn observe(redactor: &Redactor, capture: Capture, now_ms: i64) -> Observed {
    let r = redactor.release(capture).unwrap();
    Observed { now_ms, scene: r.scene().clone(), display: r.display(), app: "com.apple.Safari".into() }
}

#[test]
fn a_plan_survives_a_blinking_caret_and_a_masked_keystroke_and_dies_when_a_dialog_covers_its_target() {
    let redactor = Redactor::default();
    let policy = Policy::default();
    let mut recent = Recent::default();

    // Out: the planner sees a 1280×720 frame with the password field blacked out.
    let seen = redactor.release(login_page(false, 0, None)).unwrap();
    assert_eq!(seen.size(), (1280, 720));
    assert_eq!(seen.pixel(640, 333), [0, 0, 0, 255], "the password field, scaled, is black");
    let lease = Lease::new(seen.fingerprint(), seen.display(), 1_000);
    recent.remember(seen.scene().clone());

    // In: the caret has blinked on and the operator typed a character into the password field.
    // The frame is not the frame the planner saw, and neither plan cares.
    let now = observe(&redactor, login_page(true, 42, None), 400);
    assert_ne!(now.scene.fingerprint(), lease.frame);
    let sign_in = Action::Click { at: Point::new(500, 539), target: "the Sign in button".into() };
    let email = Action::TypeText { text: "me@example.com".into() };
    assert_eq!(policy.validate(&sign_in, &lease, &recent, &now, false), Verdict::Execute);
    assert_eq!(policy.validate(&email, &lease, &recent, &now, false), Verdict::Execute);

    // A cookie banner slides over the button before the click lands. That plan is void; the typing,
    // which goes to the focused field, is not.
    let banner = Element { role: "button".into(), label: "Accept all".into(), rect: Rect::new(700, 540, 520, 90), ..Default::default() };
    let now = observe(&redactor, login_page(true, 42, Some(banner)), 450);
    assert!(matches!(policy.validate(&sign_in, &lease, &recent, &now, false), Verdict::Refuse { .. }));
    assert_eq!(policy.validate(&email, &lease, &recent, &now, false), Verdict::Execute);
}

#[test]
fn the_operator_reaching_for_the_mouse_stops_everything_and_the_card_says_so() {
    let mut guard = Guard::new(0);
    let mut card = Activity::default();
    let seq = card.begin(&Action::TypeText { text: "tokyo weather".into() }, 400);
    card.settle(seq, Outcome::Done { note: None });

    let halt = guard
        .human(450, Human::MouseMoved { x: 0.0, y: 0.0 })
        .or(guard.human(470, Human::MouseMoved { x: 80.0, y: 0.0 }))
        .unwrap();
    card.note(halt.stop.narrate(), 470);
    assert!(!guard.may_act());
    assert_eq!(card.current().unwrap().line, "Stopped: you moved the mouse");
}
