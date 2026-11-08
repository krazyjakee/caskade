import { useLingui } from "@lingui/react/macro";
import type { PlayedOn } from "../../api/types";

/** Where a game was last played when it was another device (#74): the name
 * this device has for it, or "another device" when the two aren't paired.
 * A game last played here has no label. */
export function usePlayedOnLabel(playedOn: PlayedOn | null | undefined): string | null {
  const { t } = useLingui();
  if (!playedOn) return null;
  const device = playedOn.name?.trim();
  return device
    ? t({
        message: `Last played on ${device}`,
        comment: "Home: the paired device a game's latest session was played on",
      })
    : t({
        message: "Last played on another device",
        comment: "Home: a game's latest session was played on a device this one isn't paired with",
      });
}
