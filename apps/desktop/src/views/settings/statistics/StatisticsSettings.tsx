import { useSystemStats } from "../../../api/systemStats";
import { StatisticsDashboard } from "./StatisticsDashboard";

/** Settings → Statistics (#156): samples only while this is open and the
 * window is visible. */
export function StatisticsSettings() {
  return <StatisticsDashboard stats={useSystemStats()} />;
}
