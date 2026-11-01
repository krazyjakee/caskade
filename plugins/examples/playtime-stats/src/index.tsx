// Playtime stats (#133): a UI-only plugin. A view with the most played games
// as a bar chart, and a notice when a game session ends. The view works in
// console mode too (#152): LB/RB switch between the chart and the table.
import {
  definePlugin,
  FocusSection,
  type PluginGame,
  useGames,
  useInputAction,
} from "@caskade/sdk";
import { useState } from "react";
import "./style.css";

const hours = (seconds: number) => seconds / 3600;
const format = (seconds: number) => {
  const value = hours(seconds);
  return value >= 10 ? `${Math.round(value)} h` : `${value.toFixed(1)} h`;
};

/** One bar per game: magnitude in the theme's accent, values as text. */
function Bars({ games }: { games: PluginGame[] }) {
  const most = Math.max(...games.map((game) => game.playtimeSeconds), 1);
  return (
    <ol className="pts-bars" aria-label="Most played games, hours played">
      {games.map((game) => (
        <li key={game.id} className="pts-row">
          <span className="pts-name">{game.title}</span>
          <span className="pts-track">
            <span
              className="pts-bar"
              style={{ width: `${(game.playtimeSeconds / most) * 100}%` }}
              data-tip={`${game.title}: ${format(game.playtimeSeconds)}`}
            />
          </span>
          <span className="pts-value">{format(game.playtimeSeconds)}</span>
        </li>
      ))}
    </ol>
  );
}

function Table({ games }: { games: PluginGame[] }) {
  return (
    <table className="pts-table">
      <caption>Most played games</caption>
      <thead>
        <tr>
          <th scope="col">Game</th>
          <th scope="col">Hours played</th>
        </tr>
      </thead>
      <tbody>
        {games.map((game) => (
          <tr key={game.id}>
            <th scope="row">{game.title}</th>
            <td>{hours(game.playtimeSeconds).toFixed(1)}</td>
          </tr>
        ))}
      </tbody>
    </table>
  );
}

function Stats() {
  const top = useGames({ sort: "playtime", limit: 10 });
  const [view, setView] = useState<"chart" | "table">("chart");
  const other = () => setView((current) => (current === "chart" ? "table" : "chart"));
  useInputAction("tab.prev", other, { label: "Chart or table" });
  useInputAction("tab.next", other, { label: "Chart or table" });
  if (top.isPending) return <p className="pts-note">Loading your library…</p>;
  if (top.error)
    return (
      <p role="alert" className="pts-note">
        Couldn't read your library: {top.error.message}
      </p>
    );
  const played = (top.data ?? []).filter((game) => game.playtimeSeconds > 0);
  if (played.length === 0)
    return <p className="pts-note">No playtime yet. Play something and come back.</p>;
  const total = played.reduce((sum, game) => sum + game.playtimeSeconds, 0);
  return (
    <FocusSection id="playtime-stats" className="pts">
      <div className="pts-tile">
        <span className="pts-tile-value">{format(total)}</span>
        <span className="pts-note">across your {played.length} most played games</span>
      </div>
      <div className="pts-head">
        <h2>Most played</h2>
        <fieldset aria-label="Show as" className="pts-switch">
          {(["chart", "table"] as const).map((option) => (
            <button
              key={option}
              type="button"
              aria-pressed={view === option}
              onClick={() => setView(option)}
            >
              {option === "chart" ? "Chart" : "Table"}
            </button>
          ))}
        </fieldset>
      </div>
      {view === "chart" ? <Bars games={played} /> : <Table games={played} />}
    </FocusSection>
  );
}

export default definePlugin({
  activate(ctx) {
    ctx.slots.register("views", { id: "stats", title: "Playtime stats", component: Stats });
    ctx.events.on("game.exited", (payload) => {
      const session = payload as { gameId: string; durationMs: number };
      const minutes = Math.max(1, Math.round(session.durationMs / 60000));
      void ctx.library
        .game(session.gameId)
        .then((game) => ctx.notify(`Played ${game?.title ?? "a game"} for ${minutes}m`));
    });
  },
});
