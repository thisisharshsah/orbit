//! `orbit-look [out.png]` — one real capture, through the whole outbound boundary, with timings.
//!
//! Captures the main display, reads the frontmost app's accessibility tree and every window on
//! screen, runs on-device OCR, and hands it all to the redactor. Writes what the planner would be
//! sent — masked and downscaled — to a PNG on this machine, so a person can check the mask. Nothing
//! is sent anywhere.
#[cfg(target_os = "macos")]
fn main() {
    use orbit_daemon::{Access, Permission, Redactor, Status};
    use orbit_macos::{look, screen, MacAccess};

    let out = std::env::args().nth(1).unwrap_or_else(|| "orbit-look.png".into());
    for p in [Permission::ScreenRecording, Permission::Accessibility] {
        if MacAccess.status(p) == Status::Missing {
            eprintln!("{} is off. Run `orbit-access setup` first.", p.name());
            std::process::exit(1);
        }
    }
    let l = match look::look(&Redactor::default()) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
    };
    let (f, t) = (&l.findings, &l.timings);
    let or = |v: Option<usize>, none: &str| v.map(|n| n.to_string()).unwrap_or_else(|| none.into());
    println!("{:>5}ms  capture", t.capture);
    println!("{:>5}ms  windows       {} on screen", t.windows, or(f.windows, "could not be listed"));
    match &f.tree_incomplete {
        None => println!(
            "{:>5}ms  tree          {}: {} elements, {} password field(s){}",
            t.tree,
            f.app,
            or(f.elements, "?"),
            f.password_fields,
            f.focus.as_ref().map(|x| format!(", focus on {x}")).unwrap_or_default()
        ),
        Some(why) => println!("{:>5}ms  tree          {}: incomplete ({why})", t.tree, f.app),
    }
    println!("{:>5}ms  ocr           {} lines of text", t.ocr, or(f.text_lines, "failed:"));
    match &l.outcome {
        Ok(r) => {
            let (w, h) = r.size();
            println!("{:>5}ms  redact        {} region(s) blacked out, released at {w}×{h}", t.redact, r.masked());
            println!("{:>5}ms  total         frame {:016x}", t.total, r.fingerprint());
            match screen::save_png(r, std::path::Path::new(&out)) {
                Ok(()) => println!("\nWhat the planner would see: {out}"),
                Err(e) => eprintln!("could not save: {e}"),
            }
        }
        Err(w) => {
            println!("{:>5}ms  redact        withheld — {}", t.redact, w.narrate());
            std::process::exit(3);
        }
    }
}

#[cfg(not(target_os = "macos"))]
fn main() {
    eprintln!("orbit-look is the macOS capture check; this platform has its own driver.");
    std::process::exit(2);
}
