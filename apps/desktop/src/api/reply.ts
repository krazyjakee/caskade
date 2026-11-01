/** The core's structured error (`ErrorReport` in the bindings, #160): a stable
 * code to match on, a sentence for people, redacted details and recovery
 * hints. Declared by shape here so this file doesn't depend on which commands
 * the generated bindings have moved to it yet. */
export interface ErrorReportShape {
  code: string;
  summary: string;
  detail: string | null;
  hints: string[];
  transient: boolean;
  reference: string;
}

/** What a generated command resolves to: its data, or the core's error, a
 * structured report or (from commands not moved to it yet) a message. */
export type Reply<T> =
  | { status: "ok"; data: T }
  | { status: "error"; error: string | ErrorReportShape };

/** A failed command (or any error), as the webview shows it: `message` is the
 * summary, so code that only reads `message` keeps working. Match on `code`,
 * never on the text. */
export class CommandFailure extends Error {
  readonly code: string;
  readonly detail: string | null;
  readonly hints: string[];
  readonly transient: boolean;
  readonly reference: string | null;

  constructor(report: Partial<ErrorReportShape> & { summary: string }) {
    super(report.summary);
    this.name = "CommandFailure";
    this.code = report.code ?? "other";
    this.detail = report.detail ?? null;
    this.hints = report.hints ?? [];
    this.transient = report.transient ?? false;
    this.reference = report.reference ?? null;
  }
}

function isReport(value: unknown): value is ErrorReportShape {
  return (
    typeof value === "object" &&
    value !== null &&
    typeof (value as ErrorReportShape).code === "string" &&
    typeof (value as ErrorReportShape).summary === "string"
  );
}

/** A thrown or rejected value's text, for showing. */
export function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

/** Tauri's own answer for a command whose part of the core didn't start this
 * session ("state not managed for field …"). The one place the webview reads
 * a message's text: it is Tauri's, not Caskade's, and has no code. */
const NOT_STARTED = /state not managed/i;

/** Any thrown or rejected value as a [`CommandFailure`]. */
export function toFailure(error: unknown): CommandFailure {
  if (error instanceof CommandFailure) return error;
  if (isReport(error)) return new CommandFailure(error);
  const text = errorMessage(error);
  if (NOT_STARTED.test(text))
    return new CommandFailure({
      code: "unavailable",
      summary: "This part of Caskade didn't start this session. Restarting Caskade may fix it.",
      detail: text,
      hints: ["restart"],
    });
  return new CommandFailure({ summary: text || "Something went wrong.", detail: null });
}

/** A command's data; an error reply rejects with a [`CommandFailure`]. */
export async function unwrap<T>(reply: Promise<Reply<T>>): Promise<T> {
  const settled = await reply;
  if (settled.status === "error") throw toFailure(settled.error);
  return settled.data;
}

/** Plain-language labels for the core's recovery hints. */
export const hintLabels: Record<string, string> = {
  retry: "Trying again may work.",
  retryLater: "The other side has trouble; try again in a while.",
  checkConnection: "Check the network connection.",
  reconnectStorage: "Reconnect the drive or network share.",
  freeSpace: "Free some disk space.",
  checkPermissions: "Check that Caskade may read and write the folder.",
  checkCredentials: "Check the server address and sign-in details.",
  signIn: "Sign in to an account that may do this.",
  reload: "Reload to see the current state.",
  restart: "Restart Caskade.",
  updateCaskade: "Update Caskade to the newest version.",
};
