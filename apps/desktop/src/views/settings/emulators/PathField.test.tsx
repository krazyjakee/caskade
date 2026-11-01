import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { cleanup, render, screen } from "@testing-library/react";
import { useState } from "react";
import { afterEach, expect, it } from "vitest";
import { createEmulationFixtureApi } from "../../../api/emulationFixtures";
import { DropProvider } from "../../../drop/DropProvider";
import { fakeDrops } from "../../../drop/testing";
import { PathField } from "./EmulationFields";
import { EmulationProvider } from "./EmulationProvider";

afterEach(cleanup);

function Field() {
  const [value, setValue] = useState("");
  return <PathField label="ROM folder" value={value} onChange={setValue} />;
}

it("a path dropped on a path field fills it in", async () => {
  const drops = fakeDrops(() => screen.getByRole("textbox", { name: "ROM folder" }));
  render(
    <DropProvider source={drops.source} hitTest={drops.hitTest}>
      <QueryClientProvider client={new QueryClient()}>
        <EmulationProvider api={createEmulationFixtureApi()}>
          <Field />
        </EmulationProvider>
      </QueryClientProvider>
    </DropProvider>,
  );
  const field = screen.getByRole<HTMLInputElement>("textbox", { name: "ROM folder" });
  drops.over();
  expect(field.closest("[data-drop-active]")).toBeTruthy();
  drops.drop("/roms/snes", "/roms/n64");
  expect(field.value).toBe("/roms/snes");
});
