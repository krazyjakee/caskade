/** First-run setup's sounds. The startup sound is a recording (WAV, which
 * every webview plays without extra codecs) that plays as the welcome's
 * logo appears: Tauri's webviews allow autoplay (wry's default). Where a
 * webview wants a press first (the browser preview), it plays on the first
 * press instead. The small cues are synthesised with Web Audio. */
import startupUrl from "./sounds/startup.wav";

export type Cue = "start" | "step" | "found" | "done";

export interface Sounds {
  /** Starts audio, and the startup sound if it was refused before. Call
   * from a press. */
  unlock(): void;
  play(cue: Cue): void;
  /** Stops what is playing, when sounds are turned off. */
  silence(): void;
}

type Note = { at: number; freq: number; length: number; gain: number; type: OscillatorType };

/** Each cue as notes: times and lengths in seconds. */
const cues: Record<Exclude<Cue, "start">, Note[]> = {
  step: [{ at: 0, freq: 880, length: 0.09, gain: 0.05, type: "sine" }],
  found: [
    { at: 0, freq: 1318.5, length: 0.12, gain: 0.05, type: "triangle" },
    { at: 0.06, freq: 1760, length: 0.16, gain: 0.04, type: "sine" },
  ],
  // A major chord, spread upwards.
  done: [
    { at: 0, freq: 261.63, length: 1.8, gain: 0.09, type: "sine" },
    { at: 0.07, freq: 329.63, length: 1.7, gain: 0.07, type: "sine" },
    { at: 0.14, freq: 392, length: 1.6, gain: 0.07, type: "triangle" },
    { at: 0.21, freq: 523.25, length: 1.5, gain: 0.06, type: "sine" },
    { at: 0.3, freq: 783.99, length: 1.2, gain: 0.04, type: "sine" },
  ],
};

/** `enabled` is read on every cue, so muting takes effect at once. */
export function createSounds(enabled: () => boolean): Sounds {
  let context: AudioContext | null = null;
  let output: GainNode | null = null;
  const startup = typeof Audio === "undefined" ? null : new Audio(startupUrl);
  if (startup) startup.preload = "auto";
  // Once per setup: going back to the welcome doesn't play it again.
  let startupState: "waiting" | "played" | "refused" = "waiting";
  const playStartup = () => {
    if (!startup || startupState === "played") return;
    startupState = "played";
    startup.currentTime = 0;
    Promise.resolve(startup.play()).catch((error: unknown) => {
      // Refused without a press: try again on one. Other failures (no audio
      // output) leave setup silent.
      if (error instanceof DOMException && error.name === "NotAllowedError")
        startupState = "refused";
    });
  };
  return {
    unlock() {
      if (startupState === "refused" && enabled()) playStartup();
      if (context || typeof AudioContext === "undefined") return;
      try {
        context = new AudioContext();
        output = context.createGain();
        output.gain.value = 0.8;
        output.connect(context.destination);
      } catch {
        context = null;
      }
    },
    play(cue) {
      if (!enabled()) return;
      if (cue === "start") {
        playStartup();
        return;
      }
      if (!context || !output) return;
      if (context.state === "suspended") void context.resume();
      const now = context.currentTime + 0.01;
      for (const note of cues[cue]) {
        const oscillator = context.createOscillator();
        const envelope = context.createGain();
        oscillator.type = note.type;
        oscillator.frequency.value = note.freq;
        const start = now + note.at;
        const end = start + note.length;
        envelope.gain.setValueAtTime(0, start);
        envelope.gain.linearRampToValueAtTime(note.gain, start + Math.min(0.03, note.length / 3));
        envelope.gain.exponentialRampToValueAtTime(0.0001, end);
        oscillator.connect(envelope).connect(output);
        oscillator.start(start);
        oscillator.stop(end + 0.05);
      }
    },
    silence() {
      startup?.pause();
    },
  };
}
