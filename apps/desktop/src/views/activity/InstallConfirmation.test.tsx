/** #235: a game another device sends here is confirmed first; nothing is
 * written until then. */
import { QueryClientProvider } from "@tanstack/react-query";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { createLibraryQueryClient } from "../../api/provider";
import { InstallConfirmation } from "./InstallConfirmation";

afterEach(cleanup);

it("asks before copying a game another device sends", async () => {
  const confirm = vi.fn(async () => {});
  const dismiss = vi.fn(async () => {});
  render(
    <QueryClientProvider client={createLibraryQueryClient()}>
      <InstallConfirmation
        request={{
          requestId: "r1",
          kind: "peer",
          value: "Celeste",
          plan: null,
          peer: {
            deviceId: "a".repeat(64),
            deviceName: "Desk",
            entryId: "e1",
            gameId: null,
            title: "Celeste",
            sizeBytes: 1_200_000_000,
          },
        }}
        confirm={confirm}
        dismiss={dismiss}
      />
    </QueryClientProvider>,
  );
  expect(await screen.findByRole("heading", { name: "Copy Celeste from Desk?" })).toBeTruthy();
  expect(screen.getByText(/nothing is written until you say so/)).toBeTruthy();
  expect(confirm).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole("button", { name: "Copy here" }));
  await waitFor(() => expect(confirm).toHaveBeenCalledWith("r1"));
});
