import { i18n } from "@caskade/i18n";
import { cleanup, render, screen } from "@testing-library/react";
import { Button, ListBox, ListBoxItem, Popover, Select, SelectValue } from "react-aria-components";
import { afterEach, expect, it } from "vitest";
import type { LocaleApi } from "../api/locale";
import { LocaleProvider } from "./LocaleProvider";

afterEach(cleanup);

const api: LocaleApi = {
  current: async () => ({ locale: "sv", system: "sv", available: ["en", "sv"] }),
  subscribe: async () => () => {},
};

it("gives React Aria's own text the app's language, not the webview's", async () => {
  await i18n.loadAndActivate({ locale: "sv", messages: {} });
  render(
    <LocaleProvider api={api} initial={{ locale: "sv", system: "sv", available: ["en", "sv"] }}>
      <Select aria-label="Target">
        <Button>
          <SelectValue />
        </Button>
        <Popover>
          <ListBox>
            <ListBoxItem id="a">A</ListBoxItem>
          </ListBox>
        </Popover>
      </Select>
    </LocaleProvider>,
  );
  // An empty Select's placeholder, "Select an item" in English.
  expect(await screen.findByText("Välj en artikel")).toBeTruthy();
});
