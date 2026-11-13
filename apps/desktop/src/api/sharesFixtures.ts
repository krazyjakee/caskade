/** An in-memory one-off shares API (#247): the browser preview and tests.
 * Links that start with the fixture's prefix are known; one containing
 * `expired` has expired, one containing `stopped` was stopped, one
 * containing `relay` reaches its sharer only through a relay, and one
 * containing `save` or `collection` shares one of those (#248). */
import type { Collection_Serialize, JobSnapshot_Serialize } from "./bindings";
import type {
  ReceivedShare,
  ShareCreated,
  ShareInfo,
  ShareRecipient,
  ShareRecord,
  SharesApi,
  ShareTarget,
} from "./shares";

const DAY = 24 * 60 * 60 * 1000;
export const FIXTURE_LINK = "caskade://receive?t=caskadesharefixture";

/** A tiny QR code: what matters in tests is that one shows. */
const QR = `<?xml version="1.0"?><svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 3 3"><path d="M0 0h1v1H0zM2 2h1v1H2z"/></svg>`;

interface SharesFixtureOptions {
  shares?: ShareRecord[];
  received?: ReceivedShare[];
  /** What `inspect` answers for a known link. */
  info?: ShareInfo;
}

export function fixtureShare(overrides: Partial<ShareRecord> = {}): ShareRecord {
  return {
    id: "share-1",
    kind: "game",
    title: "Celeste",
    gameId: "celeste",
    entryId: "celeste-entry",
    size: 1_200_000_000,
    createdAtMs: Date.UTC(2026, 10, 2, 18),
    expiresAtMs: Date.UTC(2026, 10, 9, 18),
    stoppedAtMs: null,
    state: { state: "ready" },
    recipients: [
      { name: "Sam's PC", done: 600_000_000, finished: false, connected: true, serving: true },
    ],
    ...overrides,
  };
}

export function fixtureShareInfo(overrides: Partial<ShareInfo> = {}): ShareInfo {
  return {
    sharerId: "sharer",
    sharer: "Alex's Desk",
    kind: "game",
    title: "Hades",
    size: 15_000_000_000,
    windows: true,
    extras: [],
    expiresAtMs: Date.UTC(2026, 10, 9, 18),
    reach: "direct",
    contents: { kind: "game" },
    ...overrides,
  };
}

function job(title: string): JobSnapshot_Serialize {
  const now = Date.now();
  return {
    id: `share-job-${now}`,
    revision: 1,
    kind: "peerCopy",
    status: "queued",
    title,
    input: "",
    rootId: null,
    phase: "download",
    progress: null,
    warnings: [],
    question: null,
    gameId: null,
    error: null,
    candidates: [],
    selectedCandidateIndex: null,
    targetDir: null,
    dependencies: [],
    verification: null,
    failure: null,
    createdAtMs: now,
    updatedAtMs: now,
  };
}

interface SharesFixtureApi extends SharesApi {
  /** Someone opens share `id`'s link. */
  receivedBy(id: string, recipient: ShareRecipient): void;
  /** The storage the last share received went to (none: the default). */
  receivedInto(): string | undefined;
}

export function createSharesFixtureApi(options: SharesFixtureOptions = {}): SharesFixtureApi {
  let shares = options.shares ?? [];
  let received = options.received ?? [];
  const info = options.info ?? fixtureShareInfo();
  let into: string | undefined;
  const listeners = new Set<() => void>();
  const changed = () => {
    for (const listener of listeners) listener();
  };
  const find = (id: string) => {
    const share = shares.find((s) => s.id === id);
    if (!share) throw new Error("there's no such share");
    return share;
  };
  const running = (share: ShareRecord) =>
    share.state.state === "ready" || share.state.state === "preparing";
  const known = (link: string) => {
    if (!link.trim().toLowerCase().startsWith(FIXTURE_LINK)) {
      throw new Error("that isn't a Caskade share link");
    }
    if (link.includes("expired")) {
      throw new Error("This link expired on 2026-11-01. Ask whoever shared it for a new one.");
    }
    if (link.includes("stopped")) {
      throw new Error(
        "This link doesn't work any more: the share was stopped or has expired. Ask whoever shared it for a new one.",
      );
    }
  };
  return {
    async share(target: ShareTarget, expiry): Promise<ShareCreated> {
      const now = Date.now();
      const share = fixtureShare({
        id: `share-${now}`,
        kind: target.kind === "game" ? "game" : target.item.kind,
        title: target.title,
        entryId: target.kind === "game" ? target.entryId : null,
        createdAtMs: now,
        expiresAtMs: expiry === "day" ? now + DAY : expiry === "week" ? now + 7 * DAY : null,
        recipients: [],
      });
      shares = [share, ...shares];
      changed();
      return { share, link: `${FIXTURE_LINK}${share.id}` };
    },
    list: async () => shares,
    async link(id) {
      const share = find(id);
      if (!running(share)) throw new Error("this share has stopped");
      return `${FIXTURE_LINK}${id}`;
    },
    async qr(id) {
      find(id);
      return QR;
    },
    async stop(id) {
      find(id);
      shares = shares.map((s) =>
        s.id === id
          ? {
              ...s,
              stoppedAtMs: Date.now(),
              state: { state: "stopped" },
              recipients: s.recipients.map((r) => ({ ...r, connected: false, serving: false })),
            }
          : s,
      );
      changed();
    },
    async forget(id) {
      if (running(find(id))) throw new Error("stop sharing it first");
      shares = shares.filter((s) => s.id !== id);
      changed();
    },
    async inspect(link) {
      known(link);
      if (link.includes("save"))
        return { ...info, contents: { kind: "save", game: "Hades", here: "hades", files: 3 } };
      if (link.includes("collection"))
        return {
          ...info,
          title: "Co-op nights",
          contents: {
            kind: "collection",
            name: "Co-op nights",
            games: [
              { title: "Hades", igdbId: "113112", here: "installed", gameId: "fixture-1" },
              { title: "It Takes Two", igdbId: "119171", here: "missing", gameId: null },
            ],
          },
        };
      return link.includes("relay") ? { ...info, reach: "relayOnly" } : info;
    },
    async receive(link, rootId) {
      known(link);
      into = rootId;
      const started = job(info.title);
      received = [
        ...received,
        {
          job: started.id,
          title: info.title,
          sharer: info.sharer,
          receiving: true,
          keepHelping: false,
          serving: true,
          helping: 0,
        },
      ];
      changed();
      return started;
    },
    async receiveCollection(link): Promise<Collection_Serialize> {
      const { contents } = await this.inspect(link);
      if (contents.kind !== "collection") {
        throw new Error("That link shares a game: receive it instead.");
      }
      return {
        id: `collection-${Date.now()}`,
        name: contents.name,
        description: "",
        pinned: false,
        kind: { type: "manual" },
        gameIds: contents.games.flatMap((game) => (game.gameId ? [game.gameId] : [])),
        updatedAtMs: Date.now(),
      };
    },
    received: async () => received,
    async keepHelping(job, on) {
      received = received.map((r) => (r.job === job ? { ...r, keepHelping: on } : r));
      changed();
    },
    subscribe(listener) {
      listeners.add(listener);
      return () => listeners.delete(listener);
    },
    receivedInto: () => into,
    receivedBy(id, recipient) {
      shares = shares.map((s) =>
        s.id === id ? { ...s, recipients: [...s.recipients, recipient] } : s,
      );
      changed();
    },
  };
}
