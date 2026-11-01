// A TypeScript backend for the caskade:plugin@0.1.0 world (#132 spike).
// It mirrors the Rust test guest (crates/caskade-plugin-host/tests/guest).
import { emit, kvDelete, kvGet, kvSet, log, settingsGet } from "caskade:plugin/host@0.1.0";
import type { InterceptResult, PluginError } from "caskade:plugin/types@0.1.0";

const encoder = new TextEncoder();
const decoder = new TextDecoder();
const fail = (code: string, message: string): never => {
  throw { code, message } satisfies PluginError;
};

export const requests = {
  handleRequest(method: string, payload: string): string {
    switch (method) {
      case "echo":
        return payload;
      case "kv": {
        kvSet("key", encoder.encode(payload));
        const stored = kvGet("key");
        kvDelete("key");
        const gone = kvGet("key") === undefined;
        return `${stored ? decoder.decode(stored) : ""}:${gone}`;
      }
      case "emit":
        emit("greeted", payload);
        return "";
      case "settings":
        return settingsGet() ?? "null";
      case "fetch": {
        // Outbound HTTP: fetch() is async; a synchronous export can't await it.
        let answer = "pending";
        fetch(payload)
          .then((response) => {
            answer = `status ${response.status}`;
          })
          .catch((error: unknown) => {
            answer = `error ${String(error)}`;
          });
        return answer;
      }
      case "spin":
        for (;;) {}
      case "trap":
        throw new Error("the plugin failed");
      case "grow": {
        const big = new Uint8Array(256 << 20).fill(1);
        return String(big.reduce((sum, value) => sum + value, 0));
      }
      default:
        return fail("unknown_method", method);
    }
  },
};

export const hooks = {
  onEvent(name: string, payload: string): void {
    log("info", `${name} ${payload}`);
    kvSet(`event:${name}`, encoder.encode(payload));
  },
  intercept(point: string, payload: string): InterceptResult {
    if (payload.includes("cancel-me")) return { tag: "cancel", val: `${point}: not today` };
    return { tag: "continue", val: payload.replace('"program":"', '"program":"gamemoderun ') };
  },
};
