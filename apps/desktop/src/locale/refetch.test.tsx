import { pseudoLocale } from "@caskade/i18n";
import { useQuery } from "@tanstack/react-query";
import { act, cleanup, render, waitFor } from "@testing-library/react";
import { afterEach, expect, it } from "vitest";
import type { LocaleApi } from "../api/locale";
import { createLibraryQueryClient, LibraryProvider } from "../api/provider";
import type { LibraryApi } from "../api/types";
import { LocaleProvider } from "./LocaleProvider";

afterEach(cleanup);

/** What the core sent, counted each time it is asked for again. */
function Jobs({ fetched }: { fetched: string[] }) {
  useQuery({
    queryKey: ["jobs"],
    queryFn: () => {
      fetched.push("jobs");
      return fetched.length;
    },
  });
  return null;
}

it("asks the core again for what it translated when the language changes", async () => {
  let switchTo: (locale: string) => void = () => {};
  const locale: LocaleApi = {
    current: async () => ({ locale: "en", system: "en", available: ["en"] }),
    subscribe: async (listener) => {
      switchTo = listener;
      return () => {};
    },
  };
  const library = { subscribe: async () => () => {} } as unknown as LibraryApi;
  const fetched: string[] = [];
  render(
    <LocaleProvider api={locale}>
      <LibraryProvider api={library} client={createLibraryQueryClient()}>
        <Jobs fetched={fetched} />
      </LibraryProvider>
    </LocaleProvider>,
  );
  await waitFor(() => expect(fetched).toHaveLength(1));
  act(() => switchTo(pseudoLocale ?? "en-XA"));
  await waitFor(() => expect(fetched).toHaveLength(2));
});
