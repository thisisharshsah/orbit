//! `orbit-access` — what Orbit is allowed to do on this Mac, and a guided way to allow the rest.
//!
//!     orbit-access          report only; changes nothing and shows no prompts
//!     orbit-access setup    ask for whatever is missing, one permission at a time
#[cfg(target_os = "macos")]
fn main() {
    use std::io::{BufRead, IsTerminal, Write};

    use orbit_daemon::{Access, Asked, Permission, Status, Step};
    use orbit_macos::{grantee, load_asked, save_asked, settings_url, MacAccess};

    let mac = MacAccess;
    let who = grantee();
    let report = || {
        for p in Permission::ALL {
            let mark = if mac.status(p) == Status::Granted { "✓" } else { "✗" };
            println!("  {mark} {:<32} {}", p.name(), p.why());
        }
    };

    println!("Orbit needs three permissions on this Mac. macOS grants them to: {who}\n");
    report();
    println!();

    let setup = std::env::args().nth(1).as_deref() == Some("setup");
    if Asked::ready(&mac) {
        println!("All set.");
        return;
    }
    if !setup {
        println!("Run `orbit-access setup` to grant what is missing.");
        std::process::exit(1);
    }
    if !std::io::stdin().is_terminal() {
        eprintln!("setup needs a person at the keyboard: run it in a terminal.");
        std::process::exit(2);
    }

    let wait = |msg: &str| {
        print!("{msg} ");
        std::io::stdout().flush().ok();
        std::io::stdin().lock().lines().next();
    };
    let mut asked = load_asked();
    let mut relaunch = false;
    // Each permission is offered at most twice: the prompt, then settings. A person who declines
    // both is not asked a third time in the same sitting.
    let mut sent_to_settings = Vec::new();
    loop {
        let step = asked.next(&mac);
        match step {
            Step::Ready => break,
            Step::Ask(p) => {
                println!("→ {}: {}", p.name(), p.why());
                wait("  Press Enter to see the macOS prompt.");
                asked.take(step, &mac);
                if let Err(e) = save_asked(&asked) {
                    eprintln!("  (could not remember this for next time: {e})");
                }
                wait(&format!("  Choose an answer in the prompt, then press Enter. If you allowed it, switch on {who} in the list that opens."));
            }
            Step::OpenSettings(p) if sent_to_settings.contains(&p) => {
                println!("  {} is still off. Orbit will not start without it; run `orbit-access setup` again when ready.", p.name());
                break;
            }
            Step::OpenSettings(p) => {
                println!("→ {}: {}", p.name(), p.why());
                println!("  Opening System Settings → Privacy & Security → {}.", p.name());
                if !mac.open_settings(p) {
                    println!("  Could not open it; paste this into Safari: {}", settings_url(p));
                }
                sent_to_settings.push(p);
                wait(&format!("  Switch on {who} (use + if it is not listed), then press Enter."));
            }
        }
        if let Step::Ask(p) | Step::OpenSettings(p) = step {
            relaunch |= mac.needs_relaunch(p);
        }
    }

    println!();
    report();
    if relaunch && mac.status(Permission::ScreenRecording) == Status::Missing {
        println!("\nmacOS applies Screen Recording when an app starts. Quit {who} (⌘Q), reopen it, and run `orbit-access` to check.");
    } else if Asked::ready(&mac) {
        println!("\nAll set.");
    }
}

#[cfg(not(target_os = "macos"))]
fn main() {
    eprintln!("orbit-access is the macOS permission check; this platform has its own driver.");
    std::process::exit(2);
}
