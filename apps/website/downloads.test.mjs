import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";
import { downloadLinks } from "./downloads.mjs";

const snapshot = JSON.parse(await readFile(new URL("releases.json", import.meta.url), "utf8"));
const release = () => structuredClone(snapshot);

test("selects installers from new releases and ignores updater files", () => {
  const next = JSON.parse(JSON.stringify(snapshot).replaceAll(snapshot.tag_name.slice(1), "9.8.7"));
  next.assets.push(
    { name: "Caskade_9.8.7_x64.app.tar.gz" },
    { name: "Caskade_9.8.7_amd64.AppImage.sig" },
    { name: "latest.json" },
  );
  const links = downloadLinks(next);
  assert.equal(Object.keys(links).length, 5);
  for (const url of Object.values(links)) {
    assert.ok(url.startsWith("https://github.com/krazyjakee/caskade/releases/download/v9.8.7/"));
  }
  assert.ok(links.macArmUrl.endsWith("_aarch64.dmg"));
  assert.ok(links.macIntelUrl.endsWith("_x64.dmg"));
});

test("rejects missing and ambiguous installers", () => {
  const missing = release();
  missing.assets.pop();
  assert.throws(() => downloadLinks(missing), /Expected one download/);
  const duplicate = release();
  duplicate.assets.push(duplicate.assets[0]);
  assert.throws(() => downloadLinks(duplicate), /Expected one download/);
});

test("rejects unpublished releases and unexpected download URLs", () => {
  for (const flag of ["draft", "prerelease"]) {
    assert.throws(() => downloadLinks({ ...release(), [flag]: true }), /published stable release/);
  }
  const unexpected = release();
  unexpected.assets[0].browser_download_url = "https://example.com/installer.dmg";
  assert.throws(() => downloadLinks(unexpected), /Unexpected download URL/);
});
