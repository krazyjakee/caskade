/** An in-memory devices API for the browser preview and tests: this device,
 * four paired devices reached four ways, one Caskade nearby to pair, and
 * pairing that completes once confirmed. Tests drive the rest through the
 * extra methods (an incoming pairing, an opened link, a forgotten key). */
import type {
  DeviceGame,
  DeviceKind,
  DevicesStatus,
  PairedDevice,
  Pairing,
  PairingLinkTarget,
  PeerCopy,
  PlayRequest,
  Share,
  SyncApi,
  Transfer,
} from "./sync";

export interface SyncFixtureApi extends SyncApi {
  /** Another device starts a code pairing with this one. */
  incomingPairing(name: string, code: string): void;
  /** A `caskade://pair` link is opened from outside. */
  openLink(target: PairingLinkTarget): void;
  /** Copies and shares under way, as #233 and #246 will report them. */
  setTransfers(transfers: Transfer[], shares?: Share[]): void;
  /** A game's copies on the other devices (#232). */
  setCopies(gameId: string, copies: PeerCopy[]): void;
  /** A paired device's games that can be copied here (#276); each gets a
   * copy on that device too. */
  setDeviceGames(deviceId: string, games: DeviceGame[]): void;
  /** The games sent to other devices, as `gameId → deviceId`. */
  sent(): [string, string][];
  /** Another device asks to start a game here. */
  requestPlay(request: PlayRequest): void;
  /** Games started on other devices, and how requests here went. */
  played(): { started: [string, string][]; finished: [string, boolean, string?][] };
  /** The current state, for assertions. */
  state(): DevicesStatus;
}

const minute = 60_000;

function initial(now: number): DevicesStatus {
  return {
    thisDevice: {
      id: "a".repeat(64),
      name: "Den-PC",
      kind: "desktop",
      os: "linux",
      lanAddresses: ["192.168.1.20:41641"],
      internet: true,
      neverRelay: false,
      port: null,
      identity: "ready",
      listening: true,
    },
    devices: [
      {
        id: "b".repeat(64),
        name: "Steam Deck",
        kind: "handheld",
        os: "linux",
        link: "lan",
        addresses: ["192.168.1.31:41641"],
        lastSyncMs: now - 2 * minute,
        pairedAtMs: now - 30 * 24 * 60 * minute,
        syncing: false,
        syncError: null,
      },
      {
        id: "c".repeat(64),
        name: "Office laptop",
        kind: "laptop",
        os: "windows",
        link: "relay",
        addresses: [],
        lastSyncMs: now - 3 * 60 * minute,
        pairedAtMs: now - 10 * 24 * 60 * minute,
        syncing: false,
        syncError: null,
      },
      {
        id: "8".repeat(64),
        name: "Work PC",
        kind: "desktop",
        os: "windows",
        link: "direct",
        addresses: [],
        lastSyncMs: now - 20 * minute,
        pairedAtMs: now - 5 * 24 * 60 * minute,
        syncing: false,
        syncError: null,
      },
      {
        id: "d".repeat(64),
        name: "Living room",
        kind: "tv",
        os: "linux",
        link: "offline",
        addresses: ["192.168.1.40:41641"],
        lastSyncMs: now - 2 * 24 * 60 * minute,
        pairedAtMs: now - 60 * 24 * 60 * minute,
        syncing: false,
        syncError: "couldn't reach the other device",
      },
    ],
    nearby: [
      {
        id: "e".repeat(64),
        name: "Kids' PC",
        kind: "desktop",
        os: "windows",
        addresses: ["192.168.1.52:41641"],
      },
    ],
    pairing: null,
  };
}

export function createSyncFixtureApi(): SyncFixtureApi {
  const status = initial(Date.now());
  let opened: PairingLinkTarget | null = null;
  let transfers: Transfer[] = [];
  let shares: Share[] = [];
  const copies = new Map<string, PeerCopy[]>();
  const deviceGames = new Map<string, DeviceGame[]>();
  const sent: [string, string][] = [];
  const started: [string, string][] = [];
  const finished: [string, boolean, string?][] = [];
  const playListeners = new Set<(request: PlayRequest) => void>();
  const listeners = new Set<(opened?: PairingLinkTarget) => void>();
  const changed = (link?: PairingLinkTarget) => {
    for (const listener of listeners) listener(link);
  };
  const clone = () => structuredClone(status);
  const pairWith = (id: string, name: string, kind: DeviceKind) => {
    status.nearby = status.nearby.filter((device) => device.id !== id);
    status.devices = [
      ...status.devices,
      {
        id,
        name,
        kind,
        os: "windows",
        link: "lan",
        addresses: [],
        lastSyncMs: null,
        pairedAtMs: Date.now(),
        syncing: false,
        syncError: null,
      } satisfies PairedDevice,
    ];
  };
  return {
    status: async () => clone(),
    rename: async (name) => {
      status.thisDevice.name = name.trim() || "Den-PC";
      changed();
    },
    setKind: async (kind) => {
      status.thisDevice.kind = kind;
      changed();
    },
    pair: async (id) => {
      const device = status.nearby.find((nearby) => nearby.id === id);
      if (!device) throw new Error("couldn't reach the other device");
      const pairing: Pairing = {
        deviceId: id,
        name: device.name,
        kind: device.kind,
        os: device.os,
        code: "482913",
        incoming: false,
        confirmed: false,
        state: "waiting",
      };
      status.pairing = pairing;
      changed();
      return structuredClone(pairing);
    },
    confirmPairing: async (id) => {
      const pairing = status.pairing;
      if (!pairing || pairing.deviceId !== id) throw new Error("no pairing with that device");
      // The other device's user confirms at once in the preview.
      pairWith(id, pairing.name, pairing.kind);
      status.pairing = { ...pairing, confirmed: true, state: "paired" };
      changed();
    },
    cancelPairing: async (id) => {
      const pairing = status.pairing;
      if (!pairing || pairing.deviceId !== id) throw new Error("no pairing with that device");
      status.pairing = pairing.state === "waiting" ? { ...pairing, state: "cancelled" } : null;
      changed();
    },
    pairingLink: async () => ({
      url: "caskade://pair?t=caskadepairfixturelinkabcdefghijklmnopqrstuvwxyz",
      expiresAtMs: Date.now() + 10 * minute,
    }),
    acceptPairingLink: async (link) => {
      if (!link.trim().toLowerCase().includes("caskadepair")) {
        throw new Error("that isn't a Caskade pairing link");
      }
      const id = "f".repeat(64);
      pairWith(id, "Office desktop", "desktop");
      changed();
      return structuredClone(status.devices.find((device) => device.id === id) as PairedDevice);
    },
    openedLink: async () => opened,
    acceptOpenedLink: async () => {
      if (!opened) throw new Error("no pairing link is waiting");
      const id = opened.deviceId;
      opened = null;
      pairWith(id, "Office desktop", "desktop");
      changed();
      return structuredClone(status.devices.find((device) => device.id === id) as PairedDevice);
    },
    dismissOpenedLink: async () => {
      opened = null;
      changed();
    },
    forget: async (id) => {
      status.devices = status.devices.filter((device) => device.id !== id);
      changed();
    },
    syncNow: async (id) => {
      status.devices = status.devices.map((device) =>
        device.id === id ? { ...device, lastSyncMs: Date.now(), syncError: null } : device,
      );
      changed();
    },
    createIdentity: async () => {
      status.thisDevice.identity = "ready";
      status.thisDevice.listening = true;
      changed();
    },
    retryIdentity: async () => {
      changed();
    },
    copies: async (gameId) => structuredClone(copies.get(gameId) ?? []),
    copyHere: async (gameId, deviceId) => {
      const copy = copies.get(gameId)?.find((c) => c.deviceId === deviceId);
      if (!copy) throw new Error("that device has no copy of the game");
      transfers = [
        ...transfers,
        {
          id: `copy:${gameId}:${deviceId}`,
          title: gameId,
          deviceName: copy.deviceName,
          direction: "incoming",
          doneBytes: 0,
          totalBytes: copy.sizeBytes ?? 0,
          bytesPerSecond: 0,
          link: copy.link,
          state: copy.link === "relay" ? "waitingDirect" : "running",
        },
      ];
      changed();
    },
    sendTo: async (gameId, deviceId) => {
      const device = status.devices.find((d) => d.id === deviceId);
      if (!device || device.link === "offline" || device.link === "relay")
        throw new Error("couldn't reach the other device directly");
      sent.push([gameId, deviceId]);
    },
    sent: () => [...sent],
    playOn: async (gameId, deviceId) => {
      const copy = copies.get(gameId)?.find((c) => c.deviceId === deviceId);
      if (!copy) throw new Error("that game isn't in this device's library");
      if (copy.link === "offline" && !copy.wakeable)
        throw new Error(`${copy.deviceName} is out of reach, and it can't be woken from here`);
      started.push([gameId, deviceId]);
    },
    onPlayRequest: (listener) => {
      playListeners.add(listener);
      return () => playListeners.delete(listener);
    },
    finishPlay: async (requestId, ok, reason) => {
      finished.push([requestId, ok, reason]);
    },
    requestPlay: (request) => {
      for (const listener of playListeners) listener(request);
    },
    played: () => ({ started: [...started], finished: [...finished] }),
    setCopies: (gameId, next) => {
      copies.set(gameId, next);
      changed();
    },
    deviceGames: async (deviceId) => structuredClone(deviceGames.get(deviceId) ?? []),
    setDeviceGames: (deviceId, games) => {
      deviceGames.set(deviceId, games);
      const device = status.devices.find((paired) => paired.id === deviceId);
      for (const game of games) {
        const others = (copies.get(game.gameId) ?? []).filter((c) => c.deviceId !== deviceId);
        copies.set(game.gameId, [
          ...others,
          {
            deviceId,
            deviceName: device?.name ?? "",
            kind: device?.kind ?? "desktop",
            os: device?.os ?? "linux",
            link: "lan",
            sizeBytes: game.sizeBytes,
            location: "external",
            blocked: null,
            throughProton: game.throughProton,
            playing: false,
            wakeable: false,
          },
        ]);
      }
      changed();
    },
    transfers: async () => structuredClone(transfers),
    shares: async () => structuredClone(shares),
    subscribe: (listener) => {
      listeners.add(listener);
      return () => listeners.delete(listener);
    },
    incomingPairing: (name, code) => {
      status.pairing = {
        deviceId: "9".repeat(64),
        name,
        kind: "laptop",
        os: "mac",
        code,
        incoming: true,
        confirmed: false,
        state: "waiting",
      };
      changed();
    },
    openLink: (target) => {
      opened = target;
      changed(target);
    },
    setTransfers: (next, nextShares = []) => {
      transfers = next;
      shares = nextShares;
      changed();
    },
    state: () => clone(),
  };
}
