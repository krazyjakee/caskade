import type { MessageDescriptor } from "@lingui/core";
import { msg } from "@lingui/core/macro";

/** The core's job phases (`JobPhase`), as the UI names them. */
const phases: Record<string, MessageDescriptor> = {
  download: msg({ message: "Downloading", comment: "A background job's phase" }),
  install: msg({ message: "Installing", comment: "A background job's phase" }),
  move: msg({ message: "Moving", comment: "A background job's phase" }),
};

const working = msg({ message: "Working", comment: "A running job with no phase" });

/** A job's phase to show: one the core names, else (none, or a plugin's
 * own) the generic one, never the raw id. */
export function jobPhase(phase: string | undefined): MessageDescriptor {
  return (phase && Object.hasOwn(phases, phase) && phases[phase]) || working;
}
