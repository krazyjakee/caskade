import { activateLocale, pseudoLocale, sourceLocale } from "@caskade/i18n";
import { afterEach, expect, it } from "vitest";
import {
  mapJob,
  mapJobQuestion,
  type RawCandidate,
  type RawJobSnapshot,
  type RawQuestion,
} from "./installMapping";

const candidate: RawCandidate = {
  handler: "archive",
  strategy: "extract",
  confidence: 0.8,
  fidelity: "exact",
  requires: { executes_code: false, may_prompt_elevation: false },
};
it.each<RawQuestion>([
  {
    type: "choose_launch_target",
    value: { targets: [{ path: "game.exe", args: ["literal;arg"], reason: "Game executable" }] },
  },
  { type: "choose_installer", value: { artifacts: ["source:0"] } },
  {
    type: "choose_asset",
    value: { assets: [{ name: "game.zip", url: "https://example.invalid/game.zip", size: 4096 }] },
  },
  {
    type: "choose_install_directory",
    value: { paths: ["/fixture/Game"], reason: "Observed payload" },
  },
  {
    type: "reinstall_after_move",
    value: { title: "Fixture", reason: "Native paths need reinstall" },
  },
  { type: "low_confidence", value: { candidates: [candidate] } },
  { type: "confirm_execution", value: { candidates: [candidate] } },
])("maps the backend $type question to original indexes and its revision", (question) => {
  const mapped = mapJobQuestion(question, 7);
  expect(mapped.id).toBe("7");
  expect(mapped.kind).toBe(question.type);
  expect(mapped.choices).toHaveLength(1);
  expect(mapped.choices[0]?.id).toBe("0");
});
afterEach(() => activateLocale(sourceLocale));
it("says every question in the language in use", async () => {
  await activateLocale(pseudoLocale ?? "en-XA");
  const questions: RawQuestion[] = [
    { type: "choose_launch_target", value: { targets: [{ path: "g", args: [], reason: "r" }] } },
    { type: "choose_installer", value: { artifacts: ["source:0"] } },
    { type: "choose_asset", value: { assets: [{ name: "n", url: "u", size: null }] } },
    { type: "choose_install_directory", value: { paths: ["/g"], reason: "r" } },
    { type: "reinstall_after_move", value: { title: "Fixture", reason: "r" } },
    { type: "low_confidence", value: { candidates: [candidate] } },
    { type: "confirm_execution", value: { candidates: [candidate] } },
    {
      type: "choose_destination",
      value: { reason: "r", options: [{ label: "here", folder: null, copy: false }] },
    },
  ];
  // The pseudo-locale brackets all it translates.
  for (const question of questions) {
    const mapped = mapJobQuestion(question, 1);
    expect(mapped.title, question.type).toMatch(/^\[/);
    if (question.type !== "choose_destination") expect(mapped.message).toMatch(/\[/);
  }
  const strategy = mapJobQuestion({ type: "low_confidence", value: { candidates: [candidate] } }, 1)
    .choices[0];
  expect(strategy?.label).toMatch(/^\[/);
  expect(strategy?.detail).not.toMatch(/confidence|exact|installer code/);
});
it("names a torrent's payloads by name and size, not by magnet link", () => {
  const mapped = mapJobQuestion(
    {
      type: "choose_asset",
      value: {
        assets: [
          { name: "Everything", url: "magnet:?xt=urn%3Abtih%3Aab", size: 2048 },
          { name: "Game A/", url: "magnet:?xt=urn%3Abtih%3Aab&so=0-1", size: 1024 },
        ],
      },
    },
    3,
  );
  expect(mapped.title).toBe("Choose what to download");
  expect(mapped.choices.map((choice) => choice.label)).toEqual(["Everything", "Game A/"]);
  expect(mapped.choices.every((choice) => !choice.detail?.includes("magnet:"))).toBe(true);
});
it("preserves terminal diagnostics, plan selection, dependencies and target directory", () => {
  const raw: RawJobSnapshot = {
    id: "job",
    revision: 4,
    kind: "install",
    status: "failed",
    title: "Fixture",
    input: "/setup.zip",
    phase: "install",
    progress: null,
    warnings: [{ code: "warning", message: "Preserved warning" }],
    question: null,
    error: "Corrupt archive",
    gameId: null,
    targetDir: "/fixture/Game",
    candidates: [candidate],
    selectedCandidateIndex: 0,
    dependencies: [{ id: "runtime", name: "Runtime", installed: true }],
  };
  const job = mapJob(raw);
  expect(job.status).toBe("failed");
  expect(job.strategies[0]?.selected).toBe(true);
  expect(job.strategies[0]?.reason).toContain("80% confidence");
  expect(job.targetDir).toBe("/fixture/Game");
  expect(job.dependencies[0]?.installed).toBe(true);
  expect(job.error).toBe("Corrupt archive");
});
