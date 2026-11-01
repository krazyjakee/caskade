/** An account's age limit is a minimum age (`caskade_core::igdb::age`); the
 * picker offers it as the preferred board's steps. */
import { t } from "@lingui/core/macro";
import { boardName, DEFAULT_BOARD } from "../../../components/AgeRatingLabel";

/** Each board's labels and the minimum age each allows, as the core maps
 * them; labels that come to the same age are one step. */
const steps: Record<string, [label: string, age: number][]> = {
  pegi: [
    ["3", 3],
    ["7", 7],
    ["12", 12],
    ["16", 16],
    ["18", 18],
  ],
  esrb: [
    ["EC", 3],
    ["E", 6],
    ["E10+", 10],
    ["T", 13],
    ["M", 17],
    ["AO", 18],
  ],
  usk: [
    ["0", 0],
    ["6", 6],
    ["12", 12],
    ["16", 16],
    ["18", 18],
  ],
  cero: [
    ["A", 0],
    ["B", 12],
    ["C", 15],
    ["D", 17],
    ["Z", 18],
  ],
  acb: [
    ["G", 0],
    ["PG", 8],
    ["M", 15],
    ["R18+", 18],
  ],
  grac: [
    ["All", 0],
    ["12", 12],
    ["15", 15],
    ["18", 18],
  ],
  classind: [
    ["L", 0],
    ["10", 10],
    ["12", 12],
    ["14", 14],
    ["16", 16],
    ["18", 18],
  ],
};

export const NO_LIMIT = "none";

/** The age limit picker's options: no limit, then `board`'s steps, and
 * `current` when it is none of them (set under another board). Labelled in
 * the current language: call it while rendering. */
export function ageLimitOptions(board: string, current: number | null) {
  const known = board in steps ? board : DEFAULT_BOARD;
  const name = boardName(known);
  const options = [
    { id: NO_LIMIT, label: t({ message: "No limit", context: "age limit" }) },
    ...(steps[known] ?? []).map(([label, age]) => ({
      id: String(age),
      label: /^\d+$/.test(label) ? `${name} ${label}` : `${name} ${label} (${age}+)`,
    })),
  ];
  if (current !== null && !options.some((option) => option.id === String(current)))
    options.push({
      id: String(current),
      label: t({
        message: `Up to ${{ age: current }}+`,
        comment: "An age limit no step of the chosen rating board matches: 'Up to 13+'",
      }),
    });
  return options;
}

/** "PEGI 12", or "No limit". */
export function ageLimitLabel(board: string, age: number | null): string {
  return (
    ageLimitOptions(board, age).find((option) => option.id === String(age ?? NO_LIMIT))?.label ??
    t({ message: "No limit", context: "age limit" })
  );
}
