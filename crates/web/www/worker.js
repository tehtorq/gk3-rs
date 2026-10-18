// The engine, in a worker: it reads the game's files a piece at a time,
// synchronously, as only a worker may. The page passes on the player's
// mouse and keyboard.
import init, * as gk3 from "./pkg/gk3_web.js";

onmessage = async (e) => {
  const m = e.data;
  switch (m.type) {
    case "start":
      try {
        await init();
        await gk3.start(m.canvas, m.files, m.audio, m.args, m.stats, m.webgl, m.sync);
        postMessage({ status: "", started: true });
      } catch (err) {
        postMessage({ error: String(err) });
      }
      break;
    case "frame":
      gk3.frame();
      postMessage({ framed: true });
      break;
    case "moved": gk3.moved(m.x, m.y); break;
    case "mouse": gk3.mouse(m.button, m.down); break;
    case "wheel": gk3.wheel(m.dy); break;
    case "key": gk3.key(m.code, m.text, m.down, m.repeat); break;
    case "resized": gk3.resized(m.width, m.height); break;
  }
};
