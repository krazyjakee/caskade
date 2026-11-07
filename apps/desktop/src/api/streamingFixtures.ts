/** Game streaming without the core (#265): a fixture device to stream from
 * and hosting on this one, for the browser preview and tests. */
import type {
  HostOutcome,
  HostStatus,
  PairedClient,
  PairingFinished,
  PairRequest,
  StreamHost,
  StreamingApi,
  StreamingProgram,
  StreamQuality,
} from "./streaming";

export interface StreamingFixtureOptions {
  hosts?: StreamHost[];
  hosting?: HostStatus;
  /** Switching hosting on finds someone else's streaming first. */
  existing?: boolean;
  requests?: PairRequest[];
  paired?: PairedClient[];
  /** Milliseconds hosting takes to set up and start; 0 is at once. */
  setupMs?: number;
  programs?: StreamingProgram[];
  /** A device streams from this one: the host's update waits. */
  busy?: boolean;
  /** Hosts on devices paired through sync: they take the code (#267). */
  linked?: string[];
}

export const fixtureStreamHosts = (): StreamHost[] => [
  {
    id: "fixture-living-room",
    name: "Living room",
    address: "192.168.1.20",
    reach: "ready",
    paired: true,
    games: 12,
  },
  {
    id: "fixture-study",
    name: "Study",
    address: "192.168.1.31",
    reach: "ready",
    paired: false,
    games: 0,
  },
];

/** A fixture API, with what tests drive: pairings to finish and requests
 * that arrive. */
export function createStreamingFixtureApi(options: StreamingFixtureOptions = {}): StreamingApi & {
  /** Ends the pairing with `hostId` as the device would. */
  finishPairing(hostId: string, outcome: PairingFinished["outcome"], message?: string): void;
  /** A device asks to pair with this one. */
  request(request: PairRequest): void;
  /** What was entered on this device for each request. */
  accepted: { id: string; code: string; name: string }[];
  hostingCalls: boolean[];
  /** The qualities set, in order. */
  qualityCalls: StreamQuality[];
  /** Repairs asked for. */
  repairs: number;
} {
  let hosts = structuredClone(options.hosts ?? fixtureStreamHosts());
  let status: HostStatus = options.hosting ?? { state: "off", version: null, problem: null };
  let requests = structuredClone(options.requests ?? []);
  let paired = structuredClone(options.paired ?? []);
  let existing = options.existing ?? false;
  let programs = structuredClone(options.programs ?? []);
  let repairs = 0;
  const setupMs = options.setupMs ?? 0;
  const listeners = new Set<(finished: PairingFinished) => void>();
  const accepted: { id: string; code: string; name: string }[] = [];
  const hostingCalls: boolean[] = [];
  const qualityCalls: StreamQuality[] = [];
  const turnOn = (): HostOutcome => {
    status = { state: "settingUp", version: null, problem: null };
    const ready = () => {
      status = { state: "ready", version: "2026.914.233613", problem: null };
    };
    if (setupMs > 0) setTimeout(ready, setupMs);
    else ready();
    return "on";
  };
  return {
    accepted,
    hostingCalls,
    qualityCalls,
    get repairs() {
      return repairs;
    },
    finishPairing(hostId, outcome, message) {
      if (outcome === "paired") {
        hosts = hosts.map((host) => (host.id === hostId ? { ...host, paired: true } : host));
      }
      for (const listener of listeners) listener({ hostId, outcome, message: message ?? null });
    },
    request(request) {
      requests = [...requests.filter((each) => each.id !== request.id), request];
    },
    async hosts() {
      return structuredClone(hosts);
    },
    async lookAgain() {
      return structuredClone(hosts);
    },
    async addHost(address) {
      if (!/^[\w.-]+$/.test(address)) throw new Error(`${address} isn't a device's address`);
      if (!hosts.some((host) => host.address === address)) {
        hosts = [
          ...hosts,
          {
            id: `fixture-${address}`,
            name: address,
            address,
            reach: "unreachable",
            paired: false,
            games: 0,
          },
        ];
      }
      return structuredClone(hosts);
    },
    async pairWith(hostId) {
      const host = hosts.find((each) => each.id === hostId);
      if (!host) throw new Error("That device isn't on this network");
      return {
        hostId,
        hostName: host.name,
        code: "4831",
        automatic: options.linked?.includes(hostId) ?? false,
      };
    },
    onPairingFinished(listener) {
      listeners.add(listener);
      return () => listeners.delete(listener);
    },
    async forgetHost(hostId) {
      hosts = hosts.map((host) =>
        host.id === hostId ? { ...host, paired: false, games: 0 } : host,
      );
    },
    async hostStatus() {
      return structuredClone(status);
    },
    async setHosting(on) {
      hostingCalls.push(on);
      if (!on) {
        status = { state: "off", version: null, problem: null };
        return "off";
      }
      if (existing) return "existingFound";
      return turnOn();
    },
    async setQuality(quality) {
      qualityCalls.push(quality);
      const { settingsFixtureApi: settings } = await import("./settingsFixtures");
      const { value, revision } = await settings.get("core.streaming");
      return settings.set("core.streaming", { ...value, quality }, revision);
    },
    async hostThroughExisting(user, password) {
      if (!user || password !== "secret") throw new Error("That user name or password isn't right");
      existing = false;
      return turnOn();
    },
    async pairRequests() {
      return structuredClone(requests);
    },
    async acceptPairing(id, code, name) {
      const request = requests.find((each) => each.id === id);
      if (!request) throw new Error("That request has expired");
      if (!/^\d{4}$/.test(code)) throw new Error("Enter the four digits the other device shows");
      accepted.push({ id, code, name });
      requests = requests.filter((each) => each.id !== id);
      paired = [...paired, { uuid: `paired-${id}`, name }];
    },
    async declinePairing(id) {
      requests = requests.filter((each) => each.id !== id);
    },
    async pairedDevices() {
      return structuredClone(paired);
    },
    async unpair(uuid) {
      paired = paired.filter((each) => each.uuid !== uuid);
    },
    async unpairAll() {
      paired = [];
    },
    async repair() {
      repairs += 1;
      return turnOn();
    },
    async programs() {
      return structuredClone(programs);
    },
    async update(part) {
      if (part === "host" && options.busy)
        throw new Error("A device is streaming from this one. Update once the game ends.");
      programs = programs.map((program) =>
        program.part === part ? { ...program, installed: program.latest, update: false } : program,
      );
      return "on";
    },
    async skip(part) {
      programs = programs.map((program) =>
        program.part === part ? { ...program, update: false } : program,
      );
    },
  };
}
