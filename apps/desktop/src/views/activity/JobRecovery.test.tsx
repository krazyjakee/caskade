import { QueryClientProvider } from "@tanstack/react-query";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import type { JobFailure, JobSnapshot } from "../../api/install";
import { createInstallFixtureApi, fixtureJob } from "../../api/installFixtures";
import { createLibraryQueryClient } from "../../api/provider";
import { ActivityProvider } from "./ActivityProvider";
import { JobCard } from "./JobCard";

afterEach(cleanup);

function failure(overrides: Partial<JobFailure> = {}): JobFailure {
  return {
    kind: "missingSource",
    stage: "inspect",
    path: "/downloads/setup.exe",
    transient: false,
    security: "possible",
    summary:
      "“setup.exe” is no longer there (while reading the installer). Security software can cause this, but Caskade can't tell for sure.",
    detail: "No such file or directory (os error 2)",
    actions: ["redownload", "cancel"],
    occurrences: 1,
    ...overrides,
  };
}

function renderFailed(job: JobSnapshot) {
  const api = createInstallFixtureApi([job]);
  const recover = vi.spyOn(api, "recover");
  const cancel = vi.spyOn(api, "cancel");
  render(
    <QueryClientProvider client={createLibraryQueryClient()}>
      <ActivityProvider api={api}>
        <JobCard
          job={job}
          busy={false}
          cancel={vi.fn()}
          remove={vi.fn()}
          retry={vi.fn()}
          openQuestion={vi.fn()}
        />
      </ActivityProvider>
    </QueryClientProvider>,
  );
  return { recover, cancel };
}

it("explains a failure with its stage and file and recovers as the user chooses", async () => {
  const { recover } = renderFailed(
    fixtureJob({ status: "failed", error: "raw error", failure: failure() }),
  );
  expect(screen.getByRole("alert").textContent).toContain("can't tell for sure");
  expect(screen.getByText(/Reading the installer · \/downloads\/setup\.exe/)).toBeTruthy();
  expect(screen.getByText("This needs you: choose what to do next.")).toBeTruthy();
  // The failure's own actions replace the plain Retry button.
  expect(screen.queryByRole("button", { name: "Retry" })).toBeNull();
  expect(screen.queryByRole("button", { name: /Cancel/ })).toBeNull();
  fireEvent.click(screen.getByRole("button", { name: "Technical details" }));
  expect(screen.getByText(/No such file or directory/)).toBeTruthy();
  fireEvent.click(screen.getByRole("button", { name: "Download again" }));
  await waitFor(() =>
    expect(recover).toHaveBeenCalledExactlyOnceWith("fixture-job", { type: "redownload" }),
  );
});

it("installs from a located source picked by the user", async () => {
  const { recover } = renderFailed(
    fixtureJob({
      status: "failed",
      failure: failure({ actions: ["locateSource", "cancel"] }),
    }),
  );
  fireEvent.click(screen.getByRole("button", { name: "Locate the installer…" }));
  await waitFor(() =>
    expect(recover).toHaveBeenCalledExactlyOnceWith("fixture-job", {
      type: "locateSource",
      input: "/fixture/setup.zip",
    }),
  );
});

it("shows a scheduled automatic retry and lets the user stop it", async () => {
  const { cancel } = renderFailed(
    fixtureJob({
      status: "failed",
      failure: failure({
        kind: "download",
        stage: "download",
        path: undefined,
        transient: true,
        security: undefined,
        summary: "The download failed because the network or the server had trouble.",
        actions: ["retry", "redownload", "cancel"],
        retryAtMs: Date.now() + 15_000,
      }),
    }),
  );
  expect(screen.getByText(/Caskade will try again at/)).toBeTruthy();
  fireEvent.click(screen.getByRole("button", { name: "Don't retry automatically" }));
  await waitFor(() => expect(cancel).toHaveBeenCalledExactlyOnceWith("fixture-job"));
});
