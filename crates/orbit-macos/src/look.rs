//! One look at the screen, through the whole outbound boundary: capture, windows, tree, OCR, redact.
//! What `orbit-look` prints and what the live view shows are the same call.
use std::time::Instant;

use orbit_daemon::{Capture, Redactor, Released, Withheld};

use crate::{ax, ocr, screen};

/// Milliseconds spent in each stage, in order.
#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct Timings {
    pub capture: u64,
    pub windows: u64,
    pub tree: u64,
    pub ocr: u64,
    pub redact: u64,
    pub total: u64,
}

/// What the stages found, for a person to read. Counts and names only — never contents.
#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct Findings {
    pub app: String,
    pub windows: Option<usize>,
    pub elements: Option<usize>,
    pub password_fields: usize,
    pub focus: Option<String>,
    pub text_lines: Option<usize>,
    pub tree_incomplete: Option<String>,
}

pub struct Look {
    pub outcome: Result<Released, Withheld>,
    pub findings: Findings,
    pub timings: Timings,
}

pub fn look(redactor: &Redactor) -> Result<Look, String> {
    let ms = |t: Instant| t.elapsed().as_millis() as u64;
    let all = Instant::now();
    let mut timings = Timings::default();
    let mut findings = Findings::default();

    let t = Instant::now();
    let shot = screen::Shot::capture()?;
    timings.capture = ms(t);

    let t = Instant::now();
    let windows = shot.windows();
    findings.windows = windows.as_ref().map(Vec::len);
    timings.windows = ms(t);

    let t = Instant::now();
    let tree = ax::front(&shot, ax::Budget::default())?;
    timings.tree = ms(t);
    findings.app = tree.app.clone();
    findings.tree_incomplete = tree.incomplete.clone();
    if let Some(els) = &tree.elements {
        findings.elements = Some(els.len());
        findings.password_fields = els.iter().filter(|e| e.role == "secure_text_field").count();
        findings.focus = els.iter().find(|e| e.focused).map(|e| format!("{} “{}”", e.role, e.label));
    }

    let t = Instant::now();
    let text = ocr::recognise(&shot).ok();
    findings.text_lines = text.as_ref().map(Vec::len);
    timings.ocr = ms(t);

    let t = Instant::now();
    let screen::Shot { frame, .. } = shot;
    let capture = Capture { frame, app: tree.app, elements: tree.elements, windows, text };
    let outcome = redactor.release(capture);
    timings.redact = ms(t);
    timings.total = ms(all);
    Ok(Look { outcome, findings, timings })
}
