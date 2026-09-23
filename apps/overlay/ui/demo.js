/**
 * A SCRIPTED SESSION, so the overlay can be designed before there is an agent to drive it.
 *
 * Every event here is one the daemon will really send, in the order it will really send them,
 * including the two that matter most for judging the design: a step that leaves a mark, and a step
 * the agent refuses to take because the screen moved under it.
 *
 * Deleted the day the Tauri shell emits real events. Until then this is the only way to see whether
 * the card is readable while something else is moving the cursor.
 */

const t0 = Date.now();
let seq = 0;
const at = () => Date.now();

/** [delay from previous, event] */
const script = [
  [200, { kind: "state", state: "listening" }],
  [900, { kind: "note", line: "Heard: “find the weather in Tokyo and put it in the doc”" }],
  [500, { kind: "state", state: "working" }],

  [250, { kind: "begin", line: "Looking at the screen", consequence: "none" }],
  [400, { kind: "settle", outcome: "done" }],

  [350, { kind: "begin", line: "Clicking the address bar", consequence: "reversible" }],
  [450, { kind: "settle", outcome: "done" }],

  [300, { kind: "begin", line: "Typing “weather in tokyo”", consequence: "consequential" }],
  [700, { kind: "settle", outcome: "done" }],

  [250, { kind: "begin", line: "Pressing enter", consequence: "consequential" }],
  [900, { kind: "settle", outcome: "done" }],

  // The lease refusal: the page finished loading between the plan and the act.
  [500, { kind: "begin", line: "Clicking the first result", consequence: "reversible" }],
  [600, { kind: "settle", outcome: "refused", why: "Skipped: the screen changed while it was deciding" }],

  [400, { kind: "begin", line: "Looking at the screen", consequence: "none" }],
  [500, { kind: "settle", outcome: "done" }],

  [300, { kind: "begin", line: "Clicking “Tokyo, Japan — 24°C”", consequence: "reversible" }],
  [600, { kind: "settle", outcome: "done" }],

  [400, { kind: "begin", line: "Switching to Pages", consequence: "reversible" }],
  [500, { kind: "settle", outcome: "done" }],

  [300, { kind: "begin", line: "Typing 46 characters", consequence: "consequential" }],
  [800, { kind: "settle", outcome: "done" }],

  // The handback: a region of the OS the agent is not allowed to touch.
  [500, { kind: "state", state: "blocked" }],
  [100, { kind: "begin", line: "Handing back: this is a system password prompt", consequence: "none" }],
  [300, { kind: "settle", outcome: "interrupted" }],
];

async function run() {
  for (const [wait, ev] of script) {
    await new Promise((r) => setTimeout(r, wait));
    if (ev.kind === "begin" || ev.kind === "note") {
      ev.seq = ++seq;
      ev.at_ms = at();
    } else if (ev.kind === "settle") {
      ev.seq = seq; // settles the step just begun
    }
    window.ORBIT.push(ev);
  }
}

// `?still=1` freezes the script at its most interesting frame — a working card with history above it
// — so a screenshot shows the design rather than whichever instant the capture landed on.
const still = new URLSearchParams(location.search).get("still");
if (still) {
  const upTo = script.findIndex((s) => s[1].line === "Switching to Pages");
  let n = 0;
  for (const [, ev] of script.slice(0, upTo + 1)) {
    if (ev.kind === "begin" || ev.kind === "note") {
      ev.seq = ++n;
      ev.at_ms = t0 + n * 1400;
    } else if (ev.kind === "settle") {
      ev.seq = n;
    }
    window.ORBIT.push(ev);
  }
  window.ORBIT.push({ kind: "dropped", count: 12 });
} else {
  run();
}
