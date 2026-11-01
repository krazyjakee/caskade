/** Units for the Statistics dashboard: binary bytes (KiB, MiB, GiB), as
 * the Storage settings show them, and the same per second for rates. */

import { t } from "@lingui/core/macro";
import { formatBytes } from "../../../api/presentation";

export function formatRate(bytesPerSecond: number | null): string {
  return bytesPerSecond === null
    ? "—"
    : t({
        message: `${{ size: formatBytes(bytesPerSecond) }}/s`,
        comment: "A rate: bytes per second",
      });
}

/** `part` of `whole` in percent; null without a whole. */
export function percentOf(part: number, whole: number): number | null {
  return whole > 0 ? Math.min(100, Math.max(0, (part / whole) * 100)) : null;
}

/** `60 s`, `2 min`. */
export function formatWindow(ms: number): string {
  const seconds = Math.round(ms / 1000);
  const minutes = Math.round(seconds / 60);
  return seconds < 120
    ? t({ message: `${{ seconds }} s`, comment: "A span of time in seconds, abbreviated" })
    : t({ message: `${{ minutes }} min`, comment: "A span of time in minutes, abbreviated" });
}

/** A round top for a rate graph: 1, 2 or 5 × a power of ten. */
export function niceCeiling(value: number): number {
  if (value <= 0) return 1024;
  const magnitude = 10 ** Math.floor(Math.log10(value));
  for (const step of [1, 2, 5, 10]) if (value <= step * magnitude) return step * magnitude;
  return 10 * magnitude;
}
