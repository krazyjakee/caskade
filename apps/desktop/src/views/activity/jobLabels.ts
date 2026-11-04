import type { MessageDescriptor } from "@lingui/core";
import { msg } from "@lingui/core/macro";

/** The core's job kinds (`JobKind`), as the UI names them. */
const kinds: Record<string, MessageDescriptor> = {
  install: msg({ message: "Install", context: "job kind" }),
  uninstall: msg({ message: "Uninstall", context: "job kind" }),
  move: msg({ message: "Move", context: "job kind" }),
  fetch: msg({ message: "Download", context: "job kind" }),
  verify: msg({ message: "Verify", context: "job kind" }),
  update: msg({ message: "Update", context: "job kind" }),
  peerCopy: msg({
    message: "Copy from another device",
    context: "job kind",
    comment: "A game copied from another of the user's devices",
  }),
  moveLocation: msg({
    message: "Move location",
    context: "job kind",
    comment: "Every game on a storage location moved to another, then the location removed",
  }),
};

const job = msg({
  message: "Job",
  context: "job kind",
  comment: "A background job of a kind the app has no name for (a plugin's own)",
});

/** The core's job phases (`JobPhase`), as the UI names them. */
const phases: Record<string, MessageDescriptor> = {
  download: msg({ message: "Downloading", comment: "A background job's phase" }),
  install: msg({ message: "Installing", comment: "A background job's phase" }),
  move: msg({ message: "Moving", comment: "A background job's phase" }),
};

const working = msg({ message: "Working", comment: "A running job with no phase" });

const named = (labels: Record<string, MessageDescriptor>, id: string | undefined) =>
  id && Object.hasOwn(labels, id) ? labels[id] : undefined;

/** A job's kind to show: one the core names, else the generic one, never
 * the raw id. */
export function jobKind(kind: string): MessageDescriptor {
  return named(kinds, kind) ?? job;
}

/** A job's phase to show: one the core names, else (none, or a plugin's
 * own) the generic one, never the raw id. */
export function jobPhase(phase: string | undefined): MessageDescriptor {
  return named(phases, phase) ?? working;
}
