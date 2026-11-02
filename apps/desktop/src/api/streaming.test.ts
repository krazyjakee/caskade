import { describe, expect, it } from "vitest";
import { formatSource } from "./presentation";
import { type StreamHost, streamReleaseName } from "./streaming";

const den: StreamHost = {
  id: "A466B6A4-3EC7-8C2E-629E-021AC1F5E209",
  name: "Den",
  address: "192.168.1.7",
  reach: "ready",
  paired: true,
  games: 2,
};

describe("streamed releases", () => {
  it("are named by the device they stream from", () => {
    expect(streamReleaseName("stream:a466b6a4-3ec7-8c2e-629e-021ac1f5e209", [den])).toBe(
      "Stream from Den",
    );
    expect(streamReleaseName("stream:someone-else", [den])).toBe("Stream from another device");
    expect(streamReleaseName("steam", [den])).toBeUndefined();
    expect(formatSource("stream:x")).toBe("Streaming");
  });
});
