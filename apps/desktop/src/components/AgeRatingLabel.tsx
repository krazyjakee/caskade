/** A game's age rating (#204): one board's, PEGI unless an admin prefers
 * another (Settings → Accounts, `core.accounts.ratingBoard`), with every
 * board's in its tooltip. Nothing for an unrated game. The minimum age
 * behind age limits is the core's (`caskade_core::igdb::age`). */
import { useQuery } from "@tanstack/react-query";
import { useContext } from "react";
import type { IgdbAgeRating } from "../api/igdb";
import { DefaultSettingsApi, settingsKeys } from "../views/settings/useSettings";

/** The board shown when none is preferred. */
export const DEFAULT_BOARD = "pegi";
export const BOARDS = ["pegi", "esrb", "usk", "cero", "acb", "grac", "classind"] as const;
/** The settings section that holds the preferred board. */
export const ACCOUNTS_SETTINGS = "core.accounts";
const NAMES: Record<string, string> = {
  pegi: "PEGI",
  esrb: "ESRB",
  usk: "USK",
  cero: "CERO",
  acb: "ACB",
  grac: "GRAC",
  classind: "ClassInd",
};

export const boardName = (board: string) => NAMES[board] ?? board.toUpperCase();

export function ratingName(rating: IgdbAgeRating): string {
  return `${boardName(rating.board)} ${rating.rating}`;
}

/** The board an admin prefers; PEGI until the settings answer. */
export function usePreferredBoard(): string {
  const api = useContext(DefaultSettingsApi);
  const { data } = useQuery({
    queryKey: settingsKeys.section(ACCOUNTS_SETTINGS),
    queryFn: () => api.get(ACCOUNTS_SETTINGS),
    staleTime: 30_000,
    retry: false,
  });
  const board = data?.value.ratingBoard;
  return typeof board === "string" ? board : DEFAULT_BOARD;
}

/** The rating to show: `board`'s, else PEGI's, else the first known board's. */
export function shownRating(ratings: IgdbAgeRating[], board?: string): IgdbAgeRating | undefined {
  for (const wanted of [board, DEFAULT_BOARD, ...BOARDS]) {
    const found = wanted && ratings.find((rating) => rating.board === wanted);
    if (found) return found;
  }
  return ratings[0];
}

/** `board` picks the board; without it, the admin's preferred one (which
 * reads the settings, so it needs the app's query client). */
export function AgeRatingLabel({ ratings, board }: { ratings: IgdbAgeRating[]; board?: string }) {
  return board ? (
    <RatingLabel ratings={ratings} board={board} />
  ) : (
    <PreferredRatingLabel ratings={ratings} />
  );
}

function PreferredRatingLabel({ ratings }: { ratings: IgdbAgeRating[] }) {
  return <RatingLabel ratings={ratings} board={usePreferredBoard()} />;
}

function RatingLabel({ ratings, board }: { ratings: IgdbAgeRating[]; board: string }) {
  const shown = shownRating(ratings, board);
  if (!shown) return null;
  const all = ratings.map(ratingName).join(" · ");
  return (
    <span
      data-age-rating={shown.board}
      title={all}
      className="rounded-control border border-border-strong px-1.5 py-0.5 font-mono text-[11px] text-fg"
    >
      {ratingName(shown)}
    </span>
  );
}
