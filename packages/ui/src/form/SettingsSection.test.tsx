import { cleanup, render, screen } from "@testing-library/react";
import { Gauge } from "lucide-react";
import { afterEach, expect, it } from "vitest";
import { SegmentedControl } from "./SegmentedControl";
import { SettingRow, SettingsSection } from "./SettingsSection";
import { Switch } from "./Switch";

afterEach(cleanup);

it("lets a narrow row's controls wrap under its label instead of squeezing it (#41)", () => {
  const { container } = render(
    <SettingRow icon={Gauge} label="Time format" description="How times are shown.">
      <SegmentedControl
        options={[
          { id: "system", label: "By language" },
          { id: "12h", label: "12-hour" },
          { id: "24h", label: "24-hour" },
        ]}
        value="system"
        onChange={() => {}}
      />
    </SettingRow>,
  );
  const row = container.querySelector('[data-cs="setting-row"]') as HTMLElement;
  expect(row.classList).toContain("flex-wrap");
  // The icon and text share one item with a minimum width before wrapping.
  const label = row.querySelector('[data-part="label"]') as HTMLElement;
  const text = row.firstElementChild as HTMLElement;
  expect(text.contains(label)).toBe(true);
  expect(text.contains(row.querySelector('[data-part="icon"]'))).toBe(true);
  expect(text.className).toMatch(/flex-\[1_1_\d+rem\]/);
  // The controls may shrink and wrap their own items.
  const controls = row.querySelector('[data-part="controls"]') as HTMLElement;
  expect(controls.classList).not.toContain("shrink-0");
  expect(controls.classList).toContain("flex-wrap");
  expect(controls.classList).toContain("max-w-full");
  // Still labelled and described by the row.
  const group = screen.getByRole("radiogroup", { name: "Time format" });
  const description = row.querySelector('[data-part="description"]') as HTMLElement;
  expect(group.getAttribute("aria-describedby")).toContain(description.id);
});

it("shows explanations in the readable secondary foreground (#45)", () => {
  const { container } = render(
    <SettingsSection title="Startup" description="What happens when Caskade opens.">
      <SettingRow label="Launch on system startup" description="Start Caskade when you log in.">
        <Switch />
      </SettingRow>
    </SettingsSection>,
  );
  const descriptions = container.querySelectorAll('[data-part="description"]');
  expect(descriptions).toHaveLength(2);
  for (const description of descriptions) {
    expect(description.classList).toContain("text-fg-muted");
    expect(description.classList).not.toContain("text-fg-subtle");
  }
  expect(screen.getByRole("switch", { name: "Launch on system startup" })).toBeTruthy();
});
