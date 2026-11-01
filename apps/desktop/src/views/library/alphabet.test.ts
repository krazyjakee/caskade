import { i18n } from "@caskade/i18n";
import { expect, it } from "vitest";
import { alphabetOf } from "./AlphabetBar";

const at = (...letters: string[]) => letters.map((letter, offset) => ({ letter, offset }));

it("adds the language's own letters after Z in its order", () => {
  i18n.loadAndActivate({ locale: "sv", messages: {} });
  const alphabet = alphabetOf(at("A", "Z", "Ö", "Å", "Ä"));
  expect(alphabet.slice(0, 2)).toEqual(["#", "A"]);
  expect(alphabet.slice(-4)).toEqual(["Z", "Å", "Ä", "Ö"]);
  i18n.loadAndActivate({ locale: "en", messages: {} });
  expect(alphabetOf(at("#", "B"))).toEqual(["#", ..."ABCDEFGHIJKLMNOPQRSTUVWXYZ"]);
});
