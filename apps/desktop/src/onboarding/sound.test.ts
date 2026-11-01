import { afterEach, expect, it, vi } from "vitest";
import { createSounds } from "./sound";

afterEach(() => {
  vi.unstubAllGlobals();
});

/** Records what the startup recording is asked to do; `refuse` is what
 * each `play()` rejects with, in order. */
function fakeAudio(refuse: (Error | null)[] = []) {
  const calls: string[] = [];
  class FakeAudio {
    preload = "";
    currentTime = 3;
    constructor(src: string) {
      calls.push(`load ${src.split("/").pop()}`);
    }
    play() {
      calls.push(`play from ${this.currentTime}`);
      const error = refuse.shift();
      return error ? Promise.reject(error) : Promise.resolve();
    }
    pause() {
      calls.push("pause");
    }
  }
  vi.stubGlobal("Audio", FakeAudio);
  return calls;
}

const refused = () => new DOMException("needs a press", "NotAllowedError");
const settle = () => new Promise((resolve) => setTimeout(resolve));

it("plays the startup recording once, from the start, and stops when muted", () => {
  const calls = fakeAudio();
  const sounds = createSounds(() => true);
  expect(calls).toEqual(["load startup.wav"]);
  sounds.play("start");
  sounds.play("start");
  sounds.silence();
  expect(calls.slice(1)).toEqual(["play from 0", "pause"]);
});

it("stays quiet when sounds are off", () => {
  const calls = fakeAudio();
  createSounds(() => false).play("start");
  expect(calls).toEqual(["load startup.wav"]);
});

it("plays on the first press where the webview refused it before one", async () => {
  const calls = fakeAudio([refused()]);
  const sounds = createSounds(() => true);
  sounds.play("start");
  await settle();
  sounds.unlock();
  sounds.unlock();
  expect(calls.slice(1)).toEqual(["play from 0", "play from 0"]);
});

it("goes on silently where the webview can't play it at all", async () => {
  const calls = fakeAudio([new Error("no audio output")]);
  const sounds = createSounds(() => true);
  sounds.play("start");
  await settle();
  sounds.unlock();
  expect(calls.slice(1)).toEqual(["play from 0"]);
});
