# Orbit

A voice-driven computer-using agent: a dot that floats on your desktop, becomes a card while it
works, and writes down every single thing it did to your machine.

The plan and its costings are in [BLUEPRINT.md](BLUEPRINT.md). This file is what exists today and
how to run it.

## What is here

```
crates/orbit-core/     the vocabulary every other piece speaks
  action.rs            what the agent may do — a closed set, and no way to say "run this"
  activity.rs          what the operator is told: the card's history, append-only
  lease.rs             what an action was planned against, and when that stops being true
crates/orbit-daemon/   the trust boundary, as rules with no machine attached
  redact.rs            what may leave the machine: masked, downscaled, fingerprinted — or nothing
  scene.rs             what each released frame is remembered as: the elements on it, hashed tiles
  validate.rs          what may reach it: app, lease, then consequence; irreversible waits for a human
  guard.rs             when to let go: a hand on the mouse, Escape, 300ms of silence
  access.rs            the permissions it needs and why, asked for one at a time, each prompt once
crates/orbit-macos/    the macOS driver
  screen.rs            the display as RGBA, every window on it, and the menu bar and Dock
  ax.rs                the frontmost app's tree: batched reads, invisible subtrees skipped
  ocr.rs               on-device text recognition with Vision, for card numbers with no tree
  bin/orbit-access.rs  reports what is granted; `setup` walks a person through the rest
  input.rs             a listen-only tap on the operator's real keyboard and mouse, for the kill switch
  look.rs              one look: capture → windows → tree → OCR → redact, timed
  bin/orbit-look.rs    one look, saved as a PNG
  bin/orbit-watch.rs   Orbit live in your browser: the masked screen, the real card, the kill switch
apps/overlay/ui/live.* the overlay fed by orbit-watch instead of the scripted demo
apps/overlay/ui/       the dot, the card, and a scripted session to design against
```

Nothing here talks to a model yet. The macOS driver reads the screen, the accessibility tree and
on-device OCR, and the daemon redacts them; nothing is sent anywhere, and nothing moves the mouse. That is deliberate: the rules
worth getting right — a closed action set, a settled line that cannot be rewritten, a plan that
expires — are testable with no machine to drive, and they are the rules everything else will lean on.

## Run it

**See it work** (macOS) — the quickest way to understand what exists:

```sh
cargo run --release -p orbit-macos --bin orbit-watch
```

A page opens showing your main screen as the AI would receive it, refreshed every second; black never
leaves the Mac. Click into a sign-in page and watch the password box go black. Press **Let Orbit work
for 20 seconds**, take your hand off the mouse, and watch the card fill; touch the mouse or press
Escape and it stops at once, saying why.

**The core:**

```sh
cargo test          # 56 tests; one asserts the schema cannot express a shell command, another that
                    # a password field leaves the machine black
```

**Permissions** (macOS):

```sh
cargo run -p orbit-macos --bin orbit-access          # what is granted; shows no prompts
cargo run -p orbit-macos --bin orbit-access setup    # asks for the rest, one at a time
```

Then see what the planner would be sent — masked, downscaled, with per-stage timings — saved to a
PNG on this machine and nowhere else:

```sh
cargo run --release -p orbit-macos --bin orbit-look            # writes orbit-look.png
```

macOS grants these to an app, not a binary — run from a terminal, the grant goes to the terminal
(or to VS Code). The shipped daemon will be a signed `.app` so the grant is Orbit's own and a fleet
can pre-approve it by MDM profile.

**The overlay**, in a browser, with a scripted session:

```sh
cd apps/overlay/ui && python3 -m http.server 8791
# then open http://127.0.0.1:8791/index.html          — plays a session
#            http://127.0.0.1:8791/index.html?still=1 — freezes mid-task, for screenshots
```

Click the dot to read the history back when idle. Escape stops everything.

## The three decisions already made

**The action schema has no `exec`.** A planner reads a screen; screens carry text; text carries
instructions. The client cannot run a command because there is no way to express one — not a filter
that might be bypassed, an absence. Adding a capability here means arguing it against an attacker who
controls the screen.

**Every action carries a lease.** It names the frame it was planned against and expires. If what is
at the action's point changed while the model was thinking — the topmost element under a click, the
focused field for typing — the action is refused and re-planned. Not the whole screen: a caret blinks
twice a second, and a lease that demanded a still screen would refuse nearly everything. A refusal costs one
round trip; a mis-click costs whatever was under the cursor.

**The history is the product, not a debug view.** Something else is moving your mouse. The only thing
that makes that tolerable is seeing what it just did, what it is doing, and what it did while you
were reading something else — so the card is a transcript, appended to, never rewritten. Long typed
strings are recorded by length, not content, because the agent types passwords.

## Next

1. Tauri v2 shell: transparent always-on-top window, click-through while idle, the dot on the
   platform compositor rather than in the webview.
2. The rest of the macOS driver: CGEvent taps that tell the operator's input from the agent's own
   (the kill switch), then input synthesis. Capture moves to ScreenCaptureKit before
   CGDisplayCreateImage is removed. Windows second.
3. The encoder behind `Released`, and frame diffing so an unchanged screen costs nothing to resend.
