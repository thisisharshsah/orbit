//! `orbit-look [out.png]` — one real capture, through the whole outbound boundary, with timings.
//!
//! Captures the main display, reads the frontmost app's accessibility tree and every window on
//! screen, runs on-device OCR, and hands it all to the redactor. Writes what the planner would be
//! sent — masked and downscaled — to a PNG on this machine, so a person can check the mask. Nothing
//! is sent anywhere.
#[cfg(target_os = "macos")]
fn main() {
    use std::time::Instant;

    use orbit_daemon::{Access, Capture, Permission, Redactor, Status};
    use orbit_macos::{ax, ocr, screen, MacAccess};

    let out = std::env::args().nth(1).unwrap_or_else(|| "orbit-look.png".into());
    for p in [Permission::ScreenRecording, Permission::Accessibility] {
        if MacAccess.status(p) == Status::Missing {
            eprintln!("{} is off. Run `orbit-access setup` first.", p.name());
            std::process::exit(1);
        }
    }

    let ms = |t: Instant| format!("{:>5.0}ms", t.elapsed().as_secs_f64() * 1000.0);
    let all = Instant::now();

    let t = Instant::now();
    let shot = match screen::Shot::capture() {
        Ok(s) => s,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
    };
    println!("{}  capture       {}×{} at {}× scale", ms(t), shot.frame.width, shot.frame.height, shot.scale);

    let t = Instant::now();
    let windows = shot.windows();
    match &windows {
        Some(w) => println!("{}  windows       {} on screen", ms(t), w.len()),
        None => println!("{}  windows       could not be listed", ms(t)),
    }

    let t = Instant::now();
    let tree = match ax::front(&shot, ax::Budget::default()) {
        Ok(tree) => tree,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
    };
    match (&tree.elements, &tree.incomplete) {
        (Some(els), _) => {
            let secure = els.iter().filter(|e| e.role == "secure_text_field").count();
            let focus = els.iter().find(|e| e.focused).map(|e| format!(", focus on {} “{}”", e.role, e.label)).unwrap_or_default();
            println!(
                "{}  tree          {}: {} elements ({} read, {} hidden subtrees skipped), {} password field(s){focus}",
                ms(t), tree.app, els.len(), tree.visited, tree.pruned, secure
            );
        }
        (None, why) => println!(
            "{}  tree          {}: incomplete ({}; {} read, {} skipped)",
            ms(t), tree.app, why.as_deref().unwrap_or("?"), tree.visited, tree.pruned
        ),
    }

    let t = Instant::now();
    let text = match ocr::recognise(&shot) {
        Ok(spans) => {
            println!("{}  ocr           {} lines of text", ms(t), spans.len());
            Some(spans)
        }
        Err(e) => {
            println!("{}  ocr           failed: {e}", ms(t));
            None
        }
    };

    let t = Instant::now();
    let screen::Shot { frame, .. } = shot;
    let capture = Capture { frame, app: tree.app, elements: tree.elements, windows, text };
    match Redactor::default().release(capture) {
        Ok(r) => {
            let (w, h) = r.size();
            println!("{}  redact        {} region(s) blacked out, released at {w}×{h}", ms(t), r.masked());
            println!("{}  total         frame {:016x}", ms(all), r.fingerprint());
            match screen::save_png(&r, std::path::Path::new(&out)) {
                Ok(()) => println!("\nWhat the planner would see: {out}"),
                Err(e) => eprintln!("could not save: {e}"),
            }
        }
        Err(w) => {
            println!("{}  redact        withheld — {}", ms(t), w.narrate());
            std::process::exit(3);
        }
    }
}

#[cfg(not(target_os = "macos"))]
fn main() {
    eprintln!("orbit-look is the macOS capture check; this platform has its own driver.");
    std::process::exit(2);
}
