import { Button } from "@caskade/ui";
import { QueryClientProvider } from "@tanstack/react-query";
import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { useState } from "react";
import { afterEach, expect, it, vi } from "vitest";
import { mergeJobs } from "../../api/install";
import { createInstallFixtureApi, fixtureJob } from "../../api/installFixtures";
import { createLibraryQueryClient } from "../../api/provider";
import type { RootSummary, StorageApi } from "../../api/storage";
import { ActivityProvider, useFollowJob, useJobActions, useJobs } from "./ActivityProvider";
import { AddGame } from "./AddGame";
import { InstallConfirmation } from "./InstallConfirmation";
import { JobCard } from "./JobCard";
import { JobQuestionHost } from "./JobQuestionHost";
import { QuestionDialog } from "./QuestionDialog";

afterEach(cleanup);
function DialogHarness() {
  const [open, setOpen] = useState(false);
  return (
    <>
      <Button onPress={() => setOpen(true)}>Review waiting job</Button>
      {open && (
        <QuestionDialog
          question={question}
          busy={false}
          close={() => setOpen(false)}
          answer={() => {}}
        />
      )}
    </>
  );
}
const question = {
  id: "7",
  kind: "launch_target",
  title: "Choose a launch target",
  message: "Several executable files were found.",
  choices: [
    { id: "0", label: "Fixture game", detail: "Game.exe" },
    { id: "1", label: "Level editor", detail: "Editor.exe" },
  ],
};

it("shows byte progress, the target, warnings and dependencies, with strategies behind Advanced", () => {
  const retry = vi.fn();
  const result = render(
    <JobCard
      job={fixtureJob()}
      busy={false}
      cancel={vi.fn()}
      retry={retry}
      openQuestion={vi.fn()}
    />,
  );
  expect(screen.getByRole("progressbar").getAttribute("aria-valuenow")).toBe("40");
  expect(screen.getByText(/Target:.*FixtureGame/)).toBeTruthy();
  expect(screen.getByText("Package scripts were skipped.")).toBeTruthy();
  expect(screen.getByText("Fixture runtime — Required")).toBeTruthy();
  fireEvent.click(screen.getByRole("button", { name: "Advanced strategies" }));
  expect(screen.getByText("Extract archive · Selected")).toBeTruthy();
  expect(screen.queryByRole("button", { name: /Retry/ })).toBeNull();
  result.rerender(
    <JobCard
      job={fixtureJob({ status: "failed", error: "Archive is incomplete" })}
      busy={false}
      cancel={vi.fn()}
      retry={retry}
      openQuestion={vi.fn()}
    />,
  );
  fireEvent.click(screen.getByRole("button", { name: "Retry with Run installer" }));
  expect(retry).toHaveBeenCalledExactlyOnceWith("1");
  expect(screen.getByRole("alert").textContent).toBe("Archive is incomplete");
});

it("offers Retry on a failed or cancelled install, and not on other jobs", () => {
  const retry = vi.fn();
  const card = (job: Parameters<typeof fixtureJob>[0]) => (
    <JobCard
      job={fixtureJob(job)}
      busy={false}
      cancel={vi.fn()}
      retry={retry}
      openQuestion={vi.fn()}
    />
  );
  const result = render(card({ status: "failed", error: "rollback failed" }));
  fireEvent.click(screen.getByRole("button", { name: "Retry" }));
  expect(retry).toHaveBeenCalledExactlyOnceWith();
  result.rerender(card({ status: "cancelled" }));
  expect(screen.getByRole("button", { name: "Retry" })).toBeTruthy();
  for (const job of [
    { status: "completed" as const },
    { status: "running" as const },
    { status: "failed" as const, kind: "uninstall" },
  ]) {
    result.rerender(card(job));
    expect(screen.queryByRole("button", { name: "Retry" })).toBeNull();
  }
});

it("treats an unknown or zero total as indeterminate instead of reporting false completion", () => {
  render(
    <JobCard
      job={fixtureJob({ progress: { completed: 500, total: 0, unit: "files" } })}
      busy={false}
      cancel={vi.fn()}
      retry={vi.fn()}
      openQuestion={vi.fn()}
    />,
  );
  expect(screen.getByRole("progressbar").hasAttribute("aria-valuenow")).toBe(false);
});

it("requires an explicit radio choice, supports arrow keys, and keeps an errored answer visible", async () => {
  const answer = vi.fn();
  const close = vi.fn();
  const result = render(
    <QuestionDialog question={question} busy={false} close={close} answer={answer} />,
  );
  expect((screen.getByRole("button", { name: "Continue" }) as HTMLButtonElement).disabled).toBe(
    true,
  );
  const first = screen.getByRole("radio", { name: /Fixture game/ });
  act(() => first.focus());
  fireEvent.keyDown(first, { key: "ArrowDown", code: "ArrowDown" });
  fireEvent.keyUp(first, { key: "ArrowDown", code: "ArrowDown" });
  await waitFor(() =>
    expect((screen.getByRole("radio", { name: /Level editor/ }) as HTMLInputElement).checked).toBe(
      true,
    ),
  );
  fireEvent.click(screen.getByRole("button", { name: "Continue" }));
  expect(answer).toHaveBeenCalledExactlyOnceWith("1");
  result.rerender(
    <QuestionDialog
      question={question}
      busy={false}
      error="Question changed; review the latest choices"
      close={close}
      answer={answer}
    />,
  );
  expect(screen.getByRole("alert").textContent).toContain("Question changed");
  fireEvent.click(screen.getByRole("button", { name: "Not now" }));
  expect(close).toHaveBeenCalledOnce();
});

it.each(["installer", "asset", "low_confidence", "confirm_execution", "directory", "reinstall"])(
  "renders %s choices without executing or preselecting anything",
  (kind) => {
    const answer = vi.fn();
    render(
      <QuestionDialog
        question={{ ...question, kind }}
        busy={false}
        close={vi.fn()}
        answer={answer}
      />,
    );
    expect(answer).not.toHaveBeenCalled();
    expect(
      screen.getAllByRole("radio").every((radio) => !(radio as HTMLInputElement).checked),
    ).toBe(true);
  },
);

it("never lets stale events or a late list replace a newer question", () => {
  const old = fixtureJob({ revision: 1 });
  const current = fixtureJob({ revision: 3, status: "needs_user", question });
  expect(mergeJobs([current], [old, { ...old, revision: 2 }])).toEqual([current]);
  expect(mergeJobs([old], [current])).toEqual([current]);
});

it("Escape leaves the job waiting and restores focus to the review trigger", async () => {
  render(<DialogHarness />);
  const trigger = screen.getByRole("button", { name: "Review waiting job" });
  act(() => trigger.focus());
  fireEvent.click(trigger);
  const dialog = await screen.findByRole("dialog");
  fireEvent.keyDown(dialog, { key: "Escape", code: "Escape" });
  await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
  await waitFor(() => expect(document.activeElement).toBe(trigger));
});

function Connected() {
  const jobs = useJobs();
  const action = useJobActions();
  const job = jobs.data?.[0];
  return job?.question ? (
    <QuestionDialog
      question={job.question}
      busy={action.isPending}
      close={vi.fn()}
      answer={(choiceId) =>
        action.mutate({
          type: "answer",
          jobId: job.id,
          questionId: job.question?.id ?? "",
          choiceId,
        })
      }
    />
  ) : (
    <p>{job?.status}</p>
  );
}

it("submits job and question IDs, and removes the waiting state on a fresh backend snapshot", async () => {
  const api = createInstallFixtureApi([
    fixtureJob({ status: "needs_user", revision: 7, question }),
  ]);
  const answer = vi.spyOn(api, "answer");

  render(
    <QueryClientProvider client={createLibraryQueryClient()}>
      <ActivityProvider api={api}>
        <Connected />
      </ActivityProvider>
    </QueryClientProvider>,
  );
  fireEvent.click(await screen.findByRole("radio", { name: /Fixture game/ }));
  fireEvent.click(screen.getByRole("button", { name: "Continue" }));
  await screen.findByText("running");
  expect(answer).toHaveBeenCalledExactlyOnceWith("fixture-job", "7", "0");
});

it("uses native path selection and never enqueues when the chooser is cancelled", async () => {
  const api = createInstallFixtureApi([]);
  const install = vi.spyOn(api, "install");
  vi.spyOn(api, "choosePaths").mockResolvedValue([]);
  render(
    <QueryClientProvider client={createLibraryQueryClient()}>
      <ActivityProvider api={api}>
        <AddGame />
      </ActivityProvider>
    </QueryClientProvider>,
  );
  fireEvent.click(screen.getByRole("button", { name: "Add game" }));
  fireEvent.click(await screen.findByRole("menuitem", { name: "Install from file…" }));
  await waitFor(() => expect(api.choosePaths).toHaveBeenCalledWith("file"));
  expect(install).not.toHaveBeenCalled();
});

it("passes the shared picker's chosen root ID and enqueues only after selection", async () => {
  const api = createInstallFixtureApi([]);
  const install = vi.spyOn(api, "install");
  const root = (id: string): RootSummary => ({
    root: { id, path: `/fixture/${id}`, isDefault: id === "fast", label: id },
    status: "online",
    capabilities: {
      writable: true,
      executable: true,
      symlinks: true,
      caseSensitive: true,
      freeBytes: 1_000_000,
      totalBytes: 2_000_000,
      filesystem: "fixture",
      warnings: [],
    },
    folderTemplate: "{title}",
    games: [],
    gamesBytes: 0,
    otherBytes: 0,
  });
  const roots: StorageApi = {
    list: async () => [root("fast"), root("slow")],
    preferences: async () => ({ alwaysUseDefault: false }),
    add: vi.fn(),
    edit: vi.fn(),
    setDefault: vi.fn(),
    remove: vi.fn(),
    chooseDirectory: vi.fn(),
    move: vi.fn(),
    uninstall: vi.fn(),
  };
  render(
    <QueryClientProvider client={createLibraryQueryClient()}>
      <ActivityProvider api={api} storageApi={roots}>
        <AddGame />
      </ActivityProvider>
    </QueryClientProvider>,
  );
  fireEvent.click(screen.getByRole("button", { name: "Add game" }));
  fireEvent.click(await screen.findByRole("menuitem", { name: "Install from file…" }));
  const slow = await screen.findByRole("radio", { name: /slow/ });
  expect(install).not.toHaveBeenCalled();
  expect(screen.getByText(/Unknown — free space will be checked again/)).toBeTruthy();
  fireEvent.click(slow);
  fireEvent.click(screen.getByRole("button", { name: "Install here" }));
  await waitFor(() =>
    expect(install).toHaveBeenCalledExactlyOnceWith(
      { type: "path", value: "/fixture/setup.zip" },
      "slow",
    ),
  );
});

it("keeps external URLs inert until the explicit confirmation and dismisses without installing", async () => {
  const confirm = vi.fn().mockResolvedValue(undefined);
  const dismiss = vi.fn().mockResolvedValue(undefined);
  render(
    <QueryClientProvider client={createLibraryQueryClient()}>
      <InstallConfirmation
        request={{
          requestId: "request-7",
          kind: "url",
          value: "https://example.invalid/game.zip?download=1",
        }}
        confirm={confirm}
        dismiss={dismiss}
      />
    </QueryClientProvider>,
  );
  expect(confirm).not.toHaveBeenCalled();
  expect(screen.getByText(/Nothing will be downloaded/)).toBeTruthy();
  expect(screen.getByText(/asks again before running it/)).toBeTruthy();
  fireEvent.click(within(screen.getByRole("dialog")).getByRole("button", { name: "Dismiss" }));
  await waitFor(() => expect(dismiss).toHaveBeenCalledExactlyOnceWith("request-7"));
  expect(confirm).not.toHaveBeenCalled();
});

it("opens the question of a game the user added wherever they are, until they put it off", async () => {
  const api = createInstallFixtureApi([]);
  render(
    <QueryClientProvider client={createLibraryQueryClient()}>
      <ActivityProvider api={api}>
        <AddGame />
        <JobQuestionHost />
      </ActivityProvider>
    </QueryClientProvider>,
  );
  fireEvent.click(screen.getByRole("button", { name: "Add game" }));
  fireEvent.click(await screen.findByRole("menuitem", { name: "Install from link…" }));
  fireEvent.change(screen.getByRole("textbox", { name: /Download or magnet link/ }), {
    target: { value: "https://example.invalid/game" },
  });
  fireEvent.click(screen.getByRole("button", { name: "Add installation" }));
  await waitFor(() =>
    expect(screen.queryByRole("textbox", { name: /Download or magnet link/ })).toBeNull(),
  );
  const job = (await api.listJobs())[0];
  if (!job) throw new Error("no job");
  const asset = { ...question, id: "2", title: "Choose a download asset" };
  act(() => api.emit({ ...job, revision: 2, status: "needs_user", question: asset }));
  const dialog = await screen.findByRole("dialog", { name: "Choose a download asset" });
  fireEvent.click(within(dialog).getByRole("button", { name: "Not now" }));
  await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
  // A later snapshot of the same wait doesn't bring it back.
  act(() =>
    api.emit({ ...job, revision: 3, status: "needs_user", question: { ...asset, id: "3" } }),
  );
  await new Promise((resolve) => setTimeout(resolve, 50));
  expect(screen.queryByRole("dialog")).toBeNull();
});

function Follow() {
  const follow = useFollowJob();
  return <Button onPress={() => follow("fixture-job")}>Review choices</Button>;
}
it("answers a followed job's questions one after another, then lets it go", async () => {
  const api = createInstallFixtureApi([
    fixtureJob({ status: "needs_user", revision: 7, question }),
  ]);
  const answer = vi.spyOn(api, "answer");
  render(
    <QueryClientProvider client={createLibraryQueryClient()}>
      <ActivityProvider api={api}>
        <Follow />
        <JobQuestionHost />
      </ActivityProvider>
    </QueryClientProvider>,
  );
  await waitFor(async () => expect((await api.listJobs()).length).toBe(1));
  fireEvent.click(screen.getByRole("button", { name: "Review choices" }));
  fireEvent.click(await screen.findByRole("radio", { name: /Fixture game/ }));
  fireEvent.click(screen.getByRole("button", { name: "Continue" }));
  await waitFor(() => expect(answer).toHaveBeenCalledExactlyOnceWith("fixture-job", "7", "0"));
  await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
  const job = (await api.listJobs())[0];
  if (!job) throw new Error("no job");
  const next = { ...question, id: "9", title: "Run the installer program?" };
  act(() => api.emit({ ...job, revision: 9, status: "needs_user", question: next }));
  await screen.findByRole("dialog", { name: "Run the installer program?" });
  act(() => api.emit({ ...job, revision: 10, status: "completed", question: undefined }));
  await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
  act(() =>
    api.emit({ ...job, revision: 11, status: "needs_user", question: { ...next, id: "11" } }),
  );
  await new Promise((resolve) => setTimeout(resolve, 50));
  expect(screen.queryByRole("dialog")).toBeNull();
});

it("says Add game takes magnet links and .torrent files, and installs a magnet link", async () => {
  const api = createInstallFixtureApi([]);
  const install = vi.spyOn(api, "install");
  render(
    <QueryClientProvider client={createLibraryQueryClient()}>
      <ActivityProvider api={api}>
        <AddGame />
      </ActivityProvider>
    </QueryClientProvider>,
  );
  fireEvent.click(screen.getByRole("button", { name: "Add game" }));
  const file = await screen.findByRole("menuitem", { name: "Install from file…" });
  expect(file.textContent).toContain(".torrent file");
  const link = screen.getByRole("menuitem", { name: "Install from link…" });
  expect(link.textContent).toContain("magnet link");
  fireEvent.click(link);
  const magnet = "magnet:?xt=urn:btih:0123456789abcdef0123456789abcdef01234567&dn=Game";
  fireEvent.change(screen.getByRole("textbox", { name: /Download or magnet link/ }), {
    target: { value: magnet },
  });
  fireEvent.click(screen.getByRole("button", { name: "Add installation" }));
  await waitFor(() =>
    expect(install).toHaveBeenCalledWith({ type: "url", value: magnet }, undefined),
  );
});
