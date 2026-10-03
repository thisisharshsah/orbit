/**
 * THE LIVE FEED: poll `orbit-watch` for the latest frame and the activity log, and hand the log to
 * the overlay exactly as the Tauri shell will — `window.ORBIT.push(event)`, nothing else.
 *
 * Polling, not a socket: twice a second is plenty for a frame that changes once a second, and it
 * keeps the server a few dozen lines of std.
 */

const token = new URLSearchParams(location.search).get("t") ?? "";
const q = (extra = "") => `?t=${encodeURIComponent(token)}${extra}`;

const img = document.getElementById("frame");
const view = document.getElementById("view");
const stats = document.getElementById("stats");
const status = document.getElementById("status");
const work = document.getElementById("work");

let since = 0;
let frame = -1;

function stat(value, label) {
  const d = document.createElement("div");
  d.className = "stat";
  const b = document.createElement("b");
  b.textContent = value;
  const s = document.createElement("span");
  s.textContent = label;
  d.append(b, s);
  return d;
}

function render(s) {
  if (s.frame !== frame) {
    frame = s.frame;
    if (s.withheld) {
      view.innerHTML = "";
      const p = document.createElement("p");
      p.className = "withheld";
      p.textContent = `Nothing sent this time. ${s.withheld}`;
      view.append(p);
    } else {
      if (!view.contains(img)) {
        view.innerHTML = "";
        view.append(img);
      }
      img.src = `frame.png${q(`&n=${s.frame}`)}`;
    }
    const f = s.findings;
    stats.replaceChildren(
      stat(f.app || "—", "app in front (the only one shown)"),
      stat(s.withheld ? "withheld" : String(s.masked), "areas blacked out"),
      stat(String(f.password_fields), "password fields found"),
      stat(f.elements == null ? "—" : String(f.elements), "on-screen elements read"),
      stat(`${s.timings.total} ms`, "to capture and mask"),
    );
  }

  const r = s.run;
  work.disabled = r.kind !== "idle";
  status.className = r.kind === "idle" ? "" : "big";
  status.textContent =
    r.kind === "countdown"
      ? `Hands off the mouse… starting in ${r.seconds}`
      : r.kind === "working"
        ? `Orbit is working: ${r.seconds}s left. Touch the mouse or press Escape to stop it.`
        : s.last_stop
          ? `${s.last_stop}. Press the button to try again.`
          : "Then take your hand off the mouse. Touch it, or press Escape, to stop Orbit.";

  for (const ev of s.events) window.ORBIT.push(ev);
  since = s.next;
}

async function tick() {
  try {
    const res = await fetch(`state${q(`&since=${since}`)}`, { cache: "no-store" });
    if (res.ok) render(await res.json());
    else status.textContent = "orbit-watch refused this page; open the link it printed.";
  } catch {
    status.textContent = "orbit-watch is not running. Start it again in the terminal.";
  }
  setTimeout(tick, 500);
}

work.addEventListener("click", () => fetch(`work${q()}`, { method: "POST" }));
// The card's Stop button and Escape (overlay.js) end the run on the daemon, not only in the page.
window.ORBIT.onStop = () => fetch(`stop${q()}`, { method: "POST" });

tick();
