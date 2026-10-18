// The game's sound, played as the worker mixes it: chunks of interleaved
// 44.1 kHz stereo come in on a port, and every so often the player says
// back how much it still has queued, which the worker keeps topped up.
class Output extends AudioWorkletProcessor {
  constructor() {
    super();
    this.chunks = [];
    this.at = 0; // frames into the first chunk
    this.queued = 0;
    this.untold = 0;
    this.port.onmessage = (e) => {
      this.feed = e.data;
      this.feed.onmessage = (m) => {
        this.chunks.push(m.data);
        this.queued += m.data.length / 2;
      };
    };
  }

  process(_, outputs) {
    const [left, right] = outputs[0];
    for (let i = 0; i < left.length; i++) {
      const chunk = this.chunks[0];
      if (!chunk) {
        left[i] = right[i] = 0;
        continue;
      }
      left[i] = chunk[this.at * 2];
      right[i] = chunk[this.at * 2 + 1];
      this.queued--;
      if (++this.at * 2 >= chunk.length) {
        this.chunks.shift();
        this.at = 0;
      }
    }
    this.untold += left.length;
    if (this.feed && this.untold >= 1024) {
      this.untold = 0;
      this.feed.postMessage(this.queued);
    }
    return true;
  }
}

registerProcessor("gk3-output", Output);
