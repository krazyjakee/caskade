import { expect, it } from "vitest";
import { CommandFailure, toFailure, unwrap } from "./reply";

const report = {
  code: "offline",
  summary: "Can't reach romm.lan: connection refused",
  detail: "error sending request: connection refused",
  hints: ["checkConnection", "retry"],
  transient: true,
  reference: "1a2b3c4d",
};

it("rejects a structured error as a CommandFailure that keeps its code", async () => {
  const failure = await unwrap(Promise.resolve({ status: "error" as const, error: report })).catch(
    (error: unknown) => error,
  );
  expect(failure).toBeInstanceOf(CommandFailure);
  expect(failure).toMatchObject({
    message: report.summary,
    code: "offline",
    transient: true,
    reference: "1a2b3c4d",
    hints: ["checkConnection", "retry"],
  });
});

it("keeps a plain message from a command not moved to reports yet", async () => {
  await expect(
    unwrap(Promise.resolve({ status: "error" as const, error: "name is empty" })),
  ).rejects.toThrow("name is empty");
  expect(toFailure(new Error("boom")).code).toBe("other");
  expect(await unwrap(Promise.resolve({ status: "ok" as const, data: 3 }))).toBe(3);
});

it("explains a part of the core that didn't start", () => {
  const failure = toFailure(
    "state not managed for field `state` on command `storage_list`. You must call `.manage()` before using this command",
  );
  expect(failure.code).toBe("unavailable");
  expect(failure.message).toContain("didn't start");
  expect(failure.detail).toContain("storage_list");
});
