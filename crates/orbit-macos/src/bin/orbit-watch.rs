//! `orbit-watch` — Orbit running on this Mac, where you can see it.
//!
//! Opens a page in your browser showing what the AI would be sent, refreshed every second: the
//! main screen, captured, masked and shrunk here. The real overlay card sits in its corner, fed by
//! the real activity log. "Let Orbit work" arms the real kill switch for twenty seconds: touch the
//! mouse or press Escape and it stops, and the card says why.
//!
//! Nothing is sent off this machine and nothing moves the mouse. The page is served on 127.0.0.1
//! only, behind a random token, so another local app cannot read the frames without it.
#[cfg(target_os = "macos")]
mod watch {
    use std::io::{BufRead, BufReader, Write};
    use std::net::{TcpListener, TcpStream};
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    use orbit_core::{Action, Activity, Outcome};
    use orbit_daemon::{Access, Guard, Human, Permission, Redactor, Status};
    use orbit_macos::look::{self, Findings, Timings};
    use orbit_macos::{input, screen, sys, MacAccess};
    use serde_json::{json, Value};

    const PORT: u16 = 8792;
    const WORK_SECONDS: i64 = 20;
    const COUNTDOWN_SECONDS: i64 = 3;

    const PAGE: &str = include_str!("../../../../apps/overlay/ui/live.html");
    const LIVE_JS: &str = include_str!("../../../../apps/overlay/ui/live.js");
    const OVERLAY_JS: &str = include_str!("../../../../apps/overlay/ui/overlay.js");
    const OVERLAY_CSS: &str = include_str!("../../../../apps/overlay/ui/overlay.css");

    fn now_ms() -> i64 {
        SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
    }

    #[derive(Clone, Copy, PartialEq)]
    enum Run {
        Idle,
        Countdown { until: i64 },
        Working { until: i64 },
    }

    struct Live {
        png: Vec<u8>,
        frame: u64,
        masked: usize,
        withheld: Option<String>,
        findings: Findings,
        timings: Timings,
        activity: Activity,
        /// The overlay's event stream, as `window.ORBIT.push` takes it.
        events: Vec<Value>,
        guard: Guard,
        run: Run,
        last_stop: Option<String>,
    }

    impl Live {
        fn state(&mut self, state: &str) {
            self.events.push(json!({ "kind": "state", "state": state }));
        }
        fn note(&mut self, line: &str) {
            let at = now_ms();
            let seq = self.activity.note(line, at);
            self.events.push(json!({ "kind": "note", "seq": seq, "at_ms": at, "line": line }));
        }
        fn begin(&mut self, action: &Action) -> u64 {
            let at = now_ms();
            let seq = self.activity.begin(action, at);
            let e = self.activity.entries().last().expect("just begun");
            self.events.push(json!({
                "kind": "begin", "seq": seq, "at_ms": at, "line": e.line, "consequence": e.consequence,
            }));
            seq
        }
        /// Settle through the activity log, which refuses a second settlement — so a capture that
        /// finishes after the operator stopped it cannot turn "interrupted" back into "done".
        fn settle(&mut self, seq: u64, outcome: Outcome) {
            let (name, why) = match &outcome {
                Outcome::Done { note } => ("done", note.clone()),
                Outcome::Refused { why } => ("refused", Some(why.clone())),
                Outcome::Failed { why } => ("failed", Some(why.clone())),
                Outcome::Interrupted => ("interrupted", None),
                Outcome::Declined => ("declined", None),
                Outcome::Running => return,
            };
            if self.activity.settle(seq, outcome) {
                self.events.push(json!({ "kind": "settle", "seq": seq, "outcome": name, "why": why }));
            }
        }
        /// The operator took over. Interrupt whatever is in flight and say why, once.
        fn stop(&mut self, why: String) {
            if !matches!(self.run, Run::Working { .. }) {
                return;
            }
            let running: Vec<u64> = self.activity.entries().iter().filter(|e| !e.outcome.is_settled()).map(|e| e.seq).collect();
            for seq in running {
                self.settle(seq, Outcome::Interrupted);
            }
            self.note(&why);
            self.state("idle");
            self.run = Run::Idle;
            println!("  {why}");
            self.last_stop = Some(why);
        }
    }

    pub fn main() {
        for p in Permission::ALL {
            if MacAccess.status(p) == Status::Missing {
                eprintln!("{} is off. Run `orbit-access setup` first.", p.name());
                std::process::exit(1);
            }
        }
        let live = Arc::new(Mutex::new(Live {
            png: Vec::new(),
            frame: 0,
            masked: 0,
            withheld: None,
            findings: Findings::default(),
            timings: Timings::default(),
            activity: Activity::default(),
            events: Vec::new(),
            guard: Guard::new(now_ms()),
            run: Run::Idle,
            last_stop: None,
        }));

        // The kill switch, on the operator's real hands. Armed only while Orbit is working: watching
        // the screen is not acting on it, and a mirror that stopped every time you reached for the
        // mouse would be unusable.
        let l = live.clone();
        if let Err(e) = input::listen(move |at, human: Human| {
            let mut live = l.lock().unwrap();
            if matches!(live.run, Run::Working { .. })
                && let Some(halt) = live.guard.human(at, human)
            {
                live.stop(halt.stop.narrate());
            }
        }) {
            eprintln!("{e}");
            std::process::exit(1);
        }

        let l = live.clone();
        std::thread::spawn(move || capture_loop(l));

        let token = token();
        let listener = match TcpListener::bind(("127.0.0.1", PORT)) {
            Ok(l) => l,
            Err(e) => {
                eprintln!("could not open port {PORT}: {e}. Is orbit-watch already running?");
                std::process::exit(1);
            }
        };
        let url = format!("http://127.0.0.1:{PORT}/?t={token}");
        println!("Orbit is watching. Open: {url}");
        println!("Press Ctrl+C here to quit.\n");
        let open = !std::env::args().any(|a| a == "--no-open");
        if let Some(u) = sys::cfurl(&url, false).filter(|_| open) {
            // SAFETY: a live CFURL.
            unsafe { sys::LSOpenCFURLRef(u.ptr(), std::ptr::null_mut()) };
        }
        for stream in listener.incoming().flatten() {
            let (l, t) = (live.clone(), token.clone());
            std::thread::spawn(move || {
                let _ = serve(stream, &l, &t);
            });
        }
    }

    fn capture_loop(live: Arc<Mutex<Live>>) {
        let redactor = Redactor::default();
        loop {
            let started = now_ms();
            // Advance the run: countdown → working → finished.
            let seq = {
                let mut l = live.lock().unwrap();
                match l.run {
                    Run::Countdown { until } if started >= until => {
                        l.guard.resume(started);
                        l.run = Run::Working { until: started + WORK_SECONDS * 1000 };
                        l.state("working");
                        l.note("Working. Touch the mouse or press Escape to stop me.");
                        println!("  working for {WORK_SECONDS}s — touch the mouse or press Escape to stop it");
                    }
                    Run::Working { until } if started >= until => {
                        l.run = Run::Idle;
                        l.note(&format!("Finished: {WORK_SECONDS} seconds, and you never had to stop me"));
                        l.state("idle");
                        println!("  finished without being stopped");
                    }
                    _ => {}
                }
                matches!(l.run, Run::Working { .. }).then(|| l.begin(&Action::Screenshot))
            };

            let result = look::look(&redactor);

            {
                let mut l = live.lock().unwrap();
                match result {
                    Ok(look) => {
                        l.findings = look.findings;
                        l.timings = look.timings;
                        match look.outcome {
                            Ok(released) => {
                                let n = released.masked();
                                if let Ok(png) = screen::encode_png(&released) {
                                    l.png = png;
                                }
                                l.masked = n;
                                l.withheld = None;
                                if let Some(seq) = seq {
                                    let note = format!("{n} area{} blacked out", if n == 1 { "" } else { "s" });
                                    l.settle(seq, Outcome::Done { note: Some(note) });
                                }
                            }
                            Err(w) => {
                                l.withheld = Some(w.narrate());
                                if let Some(seq) = seq {
                                    l.settle(seq, Outcome::Refused { why: w.narrate() });
                                }
                            }
                        }
                    }
                    Err(e) => {
                        l.withheld = Some(e.clone());
                        if let Some(seq) = seq {
                            l.settle(seq, Outcome::Failed { why: e });
                        }
                    }
                }
                l.frame += 1;
            }
            let spent = now_ms() - started;
            std::thread::sleep(Duration::from_millis((1000 - spent).max(150) as u64));
        }
    }

    fn token() -> String {
        let mut b = [0u8; 16];
        if let Ok(mut f) = std::fs::File::open("/dev/urandom") {
            let _ = std::io::Read::read_exact(&mut f, &mut b);
        }
        b.iter().map(|x| format!("{x:02x}")).collect()
    }

    fn serve(mut stream: TcpStream, live: &Arc<Mutex<Live>>, token: &str) -> std::io::Result<()> {
        let mut first = String::new();
        let mut reader = BufReader::new(stream.try_clone()?);
        reader.read_line(&mut first)?;
        // Drain the headers; no request here has a body worth reading.
        let mut line = String::new();
        while reader.read_line(&mut line)? > 2 {
            line.clear();
        }
        let mut parts = first.split_whitespace();
        let (method, target) = (parts.next().unwrap_or(""), parts.next().unwrap_or("/"));
        let (path, query) = target.split_once('?').unwrap_or((target, ""));
        let param = |k: &str| query.split('&').find_map(|p| p.strip_prefix(&format!("{k}=")).map(str::to_string));
        let authorised = param("t").as_deref() == Some(token);

        let (status, kind, body): (&str, &str, Vec<u8>) = match (method, path) {
            ("GET", "/overlay.js") => ("200 OK", "text/javascript", OVERLAY_JS.into()),
            ("GET", "/live.js") => ("200 OK", "text/javascript", LIVE_JS.into()),
            ("GET", "/overlay.css") => ("200 OK", "text/css", OVERLAY_CSS.into()),
            _ if !authorised => ("403 Forbidden", "text/plain", b"Open the link orbit-watch printed.".to_vec()),
            ("GET", "/") => ("200 OK", "text/html; charset=utf-8", PAGE.into()),
            ("GET", "/frame.png") => ("200 OK", "image/png", live.lock().unwrap().png.clone()),
            ("GET", "/state") => {
                let since: usize = param("since").and_then(|s| s.parse().ok()).unwrap_or(0);
                let l = live.lock().unwrap();
                let now = now_ms();
                let secs = |until: i64| ((until - now).max(0) + 999) / 1000;
                let run = match l.run {
                    Run::Idle => json!({ "kind": "idle" }),
                    Run::Countdown { until } => json!({ "kind": "countdown", "seconds": secs(until) }),
                    Run::Working { until } => json!({ "kind": "working", "seconds": secs(until) }),
                };
                let events: Vec<&Value> = l.events.iter().skip(since).collect();
                let body = json!({
                    "frame": l.frame,
                    "masked": l.masked,
                    "withheld": l.withheld,
                    "findings": l.findings,
                    "timings": l.timings,
                    "run": run,
                    "last_stop": l.last_stop,
                    "events": events,
                    "next": l.events.len(),
                });
                ("200 OK", "application/json", body.to_string().into_bytes())
            }
            ("POST", "/work") => {
                let mut l = live.lock().unwrap();
                if l.run == Run::Idle {
                    l.run = Run::Countdown { until: now_ms() + COUNTDOWN_SECONDS * 1000 };
                    l.last_stop = None;
                    l.state("listening");
                    l.note("Asked to work for 20 seconds. Starting in 3: take your hand off the mouse.");
                }
                ("204 No Content", "text/plain", Vec::new())
            }
            ("POST", "/stop") => {
                live.lock().unwrap().stop("Stopped: you pressed Stop".into());
                ("204 No Content", "text/plain", Vec::new())
            }
            _ => ("404 Not Found", "text/plain", b"not here".to_vec()),
        };
        write!(
            stream,
            "HTTP/1.1 {status}\r\nContent-Type: {kind}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n",
            body.len()
        )?;
        stream.write_all(&body)
    }
}

#[cfg(target_os = "macos")]
fn main() {
    watch::main();
}

#[cfg(not(target_os = "macos"))]
fn main() {
    eprintln!("orbit-watch is the macOS live view; this platform has its own driver.");
    std::process::exit(2);
}
