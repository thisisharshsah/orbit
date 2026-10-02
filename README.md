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
apps/overlay/ui/       the dot, the card, and a scripted session to design against
```

Nothing here talks to a model, a screen or an operating system yet — the daemon included, which
takes pixels, accessibility rectangles and OCR spans from a driver that does not exist yet. That is deliberate: the rules
worth getting right — a closed action set, a settled line that cannot be rewritten, a plan that
expires — are testable with no machine to drive, and they are the rules everything else will lean on.

## Run it

**The core:**

```sh
cargo test          # 49 tests; one asserts the schema cannot express a shell command, another that
                    # a password field leaves the machine black
```

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
2. macOS driver first, because it is the machine this is being written on; Windows second. It feeds
   the daemon: ScreenCaptureKit frames, AXSecureTextField rectangles, Vision OCR spans, CGEvent taps
   that tell human input from the agent's own.
3. The encoder behind `Released`, and frame diffing so an unchanged screen costs nothing to resend.
