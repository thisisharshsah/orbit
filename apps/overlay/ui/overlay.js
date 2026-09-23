/**
 * THE OVERLAY'S ONLY LOGIC: turn a stream of events into a dot or a card.
 *
 * The contract is one function — `window.ORBIT.push(event)` — and the events mirror `orbit-core`
 * exactly, because two definitions of "what the agent is doing" would drift and the drift would
 * show as a card disagreeing with the audit log:
 *
 *   { kind: "state",  state: "idle" | "listening" | "working" | "blocked" }
 *   { kind: "begin",  seq, at_ms, line, consequence }
 *   { kind: "settle", seq, outcome: "done"|"declined"|"refused"|"failed"|"interrupted", why? }
 *   { kind: "note",   seq, at_ms, line }
 *   { kind: "dropped", count }
 *
 * No framework. This file is loaded on top of the operator's work and has to start instantly and
 * never repaint more than it must; a render loop that diffs a virtual tree to move one line is
 * cost for nothing here.
 */

const el = {
  orbit: document.getElementById("orbit"),
  dot: document.getElementById("dot"),
  badge: document.getElementById("badge"),
  headline: document.getElementById("headline"),
  history: document.getElementById("history"),
  earlier: document.getElementById("earlier"),
  stop: document.getElementById("stop"),
};

/** The glyph in the gutter. Shape first, colour second — see the CSS note. */
const MARK = {
  running: "▸",
  done: "✓",
  declined: "–", // the operator said no
  refused: "!", // the system said no — policy, a stale lease, the redactor
  failed: "✕",
  interrupted: "■",
};

/** Newest at the bottom, and only this many rows in the DOM: the card is a window on the history,
 *  not the whole of it. The full record lives in the daemon and the audit log. */
const ROWS = 40;

const rows = new Map(); // seq -> <li>
let dropped = 0;

function clock(ms) {
  const d = new Date(ms);
  return `${String(d.getHours()).padStart(2, "0")}:${String(d.getMinutes()).padStart(2, "0")}:${String(
    d.getSeconds(),
  ).padStart(2, "0")}`;
}

function setState(state) {
  el.orbit.dataset.state = state;
  el.badge.textContent =
    state === "working" ? "working" : state === "blocked" ? "needs you" : state === "listening" ? "listening" : "idle";
  // Idle closes the card unless a person opened it deliberately to read back.
  if (state === "idle" || state === "listening") el.orbit.classList.remove("open");
}

function addRow({ seq, at_ms, line, consequence = "none", outcome = "running", why }) {
  const li = document.createElement("li");
  li.dataset.outcome = outcome;
  li.dataset.consequence = consequence;
  li.innerHTML = `<span class="mark">${MARK[outcome] ?? "·"}</span><span class="line"></span><span class="when"></span>`;
  li.querySelector(".line").textContent = line;
  li.querySelector(".when").textContent = clock(at_ms);
  if (why) {
    const w = document.createElement("span");
    w.className = "why";
    w.textContent = why;
    li.querySelector(".line").append(w);
  }
  el.history.append(li);
  rows.set(seq, li);

  // Trim the DOM, not the truth.
  while (el.history.children.length > ROWS) {
    const gone = el.history.firstElementChild;
    for (const [k, v] of rows) if (v === gone) rows.delete(k);
    gone.remove();
  }
  // Only follow the tail if the reader is already at it: scrolling back through history and being
  // yanked to the bottom by the agent's next step is the most annoying thing a log can do.
  const nearBottom = el.history.scrollHeight - el.history.scrollTop - el.history.clientHeight < 48;
  if (nearBottom) el.history.scrollTop = el.history.scrollHeight;
  return li;
}

function settleRow(seq, outcome, why) {
  const li = rows.get(seq);
  if (!li) return;
  // SETTLED ONCE. The daemon enforces this too; the UI refuses as well so a late duplicate cannot
  // rewrite a line the operator already read.
  if (li.dataset.outcome !== "running") return;
  li.dataset.outcome = outcome;
  li.querySelector(".mark").textContent = MARK[outcome] ?? "·";
  if (why) {
    const w = document.createElement("span");
    w.className = "why";
    w.textContent = why;
    li.querySelector(".line").append(w);
  }
}

function headline(line) {
  el.headline.textContent = line;
}

function push(ev) {
  switch (ev.kind) {
    case "state":
      setState(ev.state);
      break;
    case "begin":
      addRow(ev);
      headline(ev.line);
      break;
    case "settle":
      settleRow(ev.seq, ev.outcome, ev.why);
      break;
    case "note":
      addRow({ ...ev, outcome: "done" });
      break;
    case "dropped":
      dropped += ev.count;
      el.earlier.textContent = dropped ? `+${dropped} earlier` : "";
      break;
    default:
      // An unknown event is a version skew between daemon and overlay. Ignored rather than thrown:
      // the overlay going blank is worse than one line not appearing.
      break;
  }
}

// Clicking the dot opens the card to read back — the history is worth having when the agent is idle,
// which is exactly when the card would otherwise be closed.
el.dot.addEventListener("click", () => el.orbit.classList.toggle("open"));
el.dot.addEventListener("keydown", (e) => {
  if (e.key === "Enter" || e.key === " ") {
    e.preventDefault();
    el.orbit.classList.toggle("open");
  }
});

// STOP IS THE LOUDEST CONTROL and also the cheapest: it does not ask the planner anything, it tells
// the daemon to drop the input tap. Escape does the same thing, everywhere, always.
function stop() {
  window.ORBIT?.onStop?.();
  push({ kind: "state", state: "idle" });
}
el.stop.addEventListener("click", stop);
window.addEventListener("keydown", (e) => {
  if (e.key === "Escape") stop();
});

window.ORBIT = { push, stop, setState };
setState("idle");
