// Hands the canvas and the game's Data files (the player's own, or the
// server's, from scripts/gk3-web) to the worker, which runs the engine and
// draws to the canvas itself, and passes on the mouse and keyboard.
const input = document.getElementById("data");
const canvas = document.getElementById("screen");
const status = document.getElementById("status");

// `?args=R25 --time 110A` starts as `gk3 play` would; none is a new game.
const args = (new URLSearchParams(location.search).get("args") ?? "")
  .split(/\s+/)
  .filter((a) => a);

// Keys the browser would otherwise act on itself.
const OURS = new Set([
  "Tab", "Space", "Backspace", "F1", "F5", "F9", "ArrowUp", "ArrowDown",
  "ArrowLeft", "ArrowRight", "PageUp", "PageDown", "Escape", "Enter",
]);

// The page's sound output: a worklet playing what the worker mixes, at
// the game's 44.1 kHz. Gives back the worker's end of the way to it. The
// browser lets it start only once the player has clicked or pressed a key.
function sound() {
  if (!window.AudioContext || !window.AudioWorkletNode) return null;
  let context;
  try {
    context = new AudioContext({ sampleRate: 44100, latencyHint: "interactive" });
  } catch (err) {
    console.error("no sound:", err);
    return null;
  }
  const channel = new MessageChannel();
  context.audioWorklet.addModule("audio.js").then(() => {
    const node = new AudioWorkletNode(context, "gk3-output", { outputChannelCount: [2] });
    node.connect(context.destination);
    node.port.postMessage(channel.port1, [channel.port1]);
  }).catch((err) => console.error("no sound:", err));
  const resume = () => {
    if (context.state !== "running") context.resume();
  };
  window.addEventListener("pointerdown", resume);
  window.addEventListener("keydown", resume);
  return channel.port2;
}

// Starts the game from `files`, the player's picked Data files, or with
// none, from the Data folder the server has beside the page.
function begin(files) {
  document.getElementById("start").hidden = true;
  canvas.hidden = false;
  const scale = () => window.devicePixelRatio || 1;
  const size = () => [
    Math.max(1, Math.round(canvas.clientWidth * scale())),
    Math.max(1, Math.round(canvas.clientHeight * scale())),
  ];
  [canvas.width, canvas.height] = size();
  const offscreen = canvas.transferControlToOffscreen();
  const worker = new Worker("worker.js", { type: "module" });
  const send = (m, transfer = []) => worker.postMessage(m, transfer);
  worker.onmessage = (e) => {
    const m = e.data;
    if ("cursor" in m) canvas.style.cursor = m.cursor;
    else if (m.quit) status.textContent = "The game has ended.";
    else status.textContent = m.error ?? m.status ?? "";
  };
  status.textContent = "Starting…";
  const audio = sound();
  send({ type: "start", canvas: offscreen, files, audio, args }, audio ? [offscreen, audio] : [offscreen]);

  // The pointer, in the canvas's own pixels.
  const at = (e) => {
    const r = canvas.getBoundingClientRect();
    return { x: (e.clientX - r.left) * scale(), y: (e.clientY - r.top) * scale() };
  };
  canvas.addEventListener("pointermove", (e) => send({ type: "moved", ...at(e) }));
  canvas.addEventListener("pointerdown", (e) => {
    canvas.setPointerCapture(e.pointerId);
    send({ type: "moved", ...at(e) });
    send({ type: "mouse", button: e.button, down: true });
    e.preventDefault();
  });
  canvas.addEventListener("pointerup", (e) => {
    send({ type: "moved", ...at(e) });
    send({ type: "mouse", button: e.button, down: false });
  });
  canvas.addEventListener("contextmenu", (e) => e.preventDefault());
  canvas.addEventListener("wheel", (e) => {
    const lines = e.deltaMode === 1 ? 40 : e.deltaMode === 2 ? 800 : 1;
    send({ type: "wheel", dy: -e.deltaY * lines });
    e.preventDefault();
  }, { passive: false });

  const key = (down) => (e) => {
    // F11 is the browser's own fullscreen, which only the page may ask for.
    if (e.code === "F11") {
      if (down && !e.repeat) {
        if (document.fullscreenElement) document.exitFullscreen();
        else canvas.requestFullscreen();
      }
      e.preventDefault();
      return;
    }
    if (OURS.has(e.code) || e.ctrlKey || e.altKey) e.preventDefault();
    const text = e.key.length === 1 ? e.key : null;
    send({ type: "key", code: e.code, text, down, repeat: e.repeat });
  };
  window.addEventListener("keydown", key(true));
  window.addEventListener("keyup", key(false));

  new ResizeObserver(() => {
    const [width, height] = size();
    send({ type: "resized", width, height });
  }).observe(canvas);
}

input.addEventListener("change", () => {
  // Only what lies in the Data folder itself, not in folders under it.
  const files = Array.from(input.files).filter(
    (f) => f.webkitRelativePath.split("/").length === 2,
  );
  if (!files.some((f) => f.name.toLowerCase().endsWith(".brn"))) {
    status.textContent = "That folder has no .brn archives: choose the game's Data folder.";
    return;
  }
  begin(files);
});

// Served by scripts/gk3-web, the Data folder is there already: no need to
// ask for it. Otherwise the player picks it.
fetch("data/index.json")
  .then((r) => (r.ok ? begin(null) : (document.getElementById("start").hidden = false)))
  .catch(() => (document.getElementById("start").hidden = false));
