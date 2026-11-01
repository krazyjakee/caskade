import type { Meta } from "@storybook/react-vite";
import type { SystemStatsState } from "../../../../desktop/src/api/systemStats";
import {
  fixtureStatsState,
  type StatsScenario,
} from "../../../../desktop/src/api/systemStatsFixtures";
import { StatisticsDashboard } from "../../../../desktop/src/views/settings/statistics/StatisticsDashboard";
import type { Story } from "../../catalogue/stories";

/** Settings → Statistics (#156), the app's own dashboard over made-up
 * machines (`apps/desktop/src/api/systemStatsFixtures.ts`): two GPUs, four
 * volumes and several interfaces; a machine with nothing to report; failed
 * samples; and the loading, stale and error states. The Mode toolbar, or
 * the console stories, show it in console mode. */
const meta = { title: "Shell/Statistics" } satisfies Meta;
export default meta;

function Dashboard({
  scenario = "normal",
  state = {},
}: {
  scenario?: StatsScenario;
  state?: Partial<SystemStatsState>;
}) {
  return (
    <div className="max-w-5xl">
      <StatisticsDashboard stats={fixtureStatsState(scenario, state)} />
    </div>
  );
}

export const Desktop: Story = { render: () => <Dashboard /> };
export const Console: Story = { render: () => <Dashboard />, globals: { mode: "console" } };
export const NothingToReport: Story = { render: () => <Dashboard scenario="unavailable" /> };
export const FailedSamples: Story = {
  render: () => <Dashboard scenario="failing" />,
};
export const FailedSamplesConsole: Story = {
  render: () => <Dashboard scenario="failing" />,
  globals: { mode: "console" },
};
export const Stale: Story = { render: () => <Dashboard state={{ status: "stale" }} /> };
export const Loading: Story = {
  render: () => <Dashboard state={{ status: "loading", samples: [] }} />,
};
export const Unavailable: Story = {
  render: () => (
    <Dashboard
      state={{
        status: "error",
        samples: [],
        error: "Open Caskade desktop to see this computer's statistics.",
      }}
    />
  ),
};
