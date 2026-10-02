//! One step of the loop, across the boundary in both directions: a frame out, a plan back, an
//! action checked against the screen as it is now, and the operator's card told what happened.
use orbit_core::{Action, Activity, Lease, Outcome, Point};
use orbit_daemon::{Capture, Element, Frame, Guard, Human, Observed, Policy, Rect, Redactor, Verdict};

fn login_page(typed: u8) -> Capture {
    let mut px = vec![240u8; 1920 * 1080 * 4];
    // Whatever is typed into the password field changes pixels inside it, and only there.
    px[(500 * 1920 + 900) * 4] = typed;
    Capture {
        frame: Frame::new(1920, 1080, 0, px).unwrap(),
        app: "com.apple.Safari".into(),
        elements: vec![Element { role: "secure_text_field".into(), rect: Rect::new(800, 480, 320, 40) }],
        text: Some(vec![]),
    }
}

#[test]
fn a_planned_click_survives_typing_in_a_masked_field_and_dies_when_the_operator_grabs_the_mouse() {
    let redactor = Redactor::default();
    let policy = Policy::default();
    let mut guard = Guard::new(0);
    let mut card = Activity::default();

    // Out: the planner sees a 1280×720 frame with the field blacked out, and plans against it.
    let seen = redactor.release(login_page(0)).unwrap();
    assert_eq!(seen.size(), (1280, 720));
    assert_eq!(seen.pixel(640, 333), [0, 0, 0, 255], "the field, scaled, is black");
    let lease = Lease::new(seen.fingerprint(), seen.display(), 1_000);
    let action = Action::Click { at: Point::new(500, 600), target: "the Sign in button".into() };

    // In, 400ms later: the operator has typed a character, which the planner could never have seen
    // and the lease therefore does not care about.
    guard.heard(400);
    let now = redactor.release(login_page(42)).unwrap();
    let observed = Observed { now_ms: 400, frame: now.fingerprint(), display: now.display(), app: "com.apple.Safari".into() };
    assert!(guard.may_act());
    assert_eq!(policy.validate(&action, &lease, &observed, false), Verdict::Execute);
    let seq = card.begin(&action, 400);
    card.settle(seq, Outcome::Done { note: None });

    // The operator reaches for the mouse. Nothing else runs, and the card says why.
    let halt = guard.human(450, Human::MouseMoved { x: 0.0, y: 0.0 })
        .or(guard.human(470, Human::MouseMoved { x: 80.0, y: 0.0 }))
        .unwrap();
    card.note(halt.stop.narrate(), 470);
    assert!(!guard.may_act());
    assert_eq!(card.current().unwrap().line, "Stopped: you moved the mouse");
}
