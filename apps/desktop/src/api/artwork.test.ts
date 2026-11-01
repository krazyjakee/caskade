import { expect, it } from "vitest";
import { originalPath, thumbnailPath } from "./artwork";

it("selects exact grid/list/detail thumbnails and rejects originals or escaped cache paths", () => {
  const prefix = "a".repeat(64);
  const artwork = {
    thumbnails: [96, 320, 960].map((size) => ({ size, path: `${prefix}/thumb-${size}.webp` })),
  };
  expect(thumbnailPath(artwork, 320)).toBe(`${prefix}/thumb-320.webp`);
  expect(thumbnailPath(artwork, 96)).toBe(`${prefix}/thumb-96.webp`);
  expect(thumbnailPath(artwork, 960)).toBe(`${prefix}/thumb-960.webp`);
  expect(thumbnailPath({ thumbnails: [] }, 320)).toBeNull();
  for (const path of [
    `${prefix}/original.webp`,
    "../thumb-320.webp",
    "/home/user/cover.webp",
    "https://example.test/cover.webp",
  ]) {
    expect(thumbnailPath({ thumbnails: [{ size: 320, path }] }, 320)).toBeNull();
  }
});

it("selects the cached original only by its exact cache path", () => {
  const prefix = "b".repeat(64);
  expect(originalPath({ cachedPath: `${prefix}/original.webp` })).toBe(`${prefix}/original.webp`);
  expect(originalPath({})).toBeNull();
  for (const cachedPath of [
    `${prefix}/thumb-960.webp`,
    "../original.webp",
    "/home/user/original.webp",
    `${prefix}/../original.webp`,
  ]) {
    expect(originalPath({ cachedPath })).toBeNull();
  }
});
