// HowLongToBeat (#133): a game-details tab with times from the backend half.
import { definePlugin, useBackendQuery, useGame } from "@caskade/sdk";
import "./style.css";

interface Times {
  title: string;
  mainHours: number | null;
  extrasHours: number | null;
  completionistHours: number | null;
}

const hours = (value: number | null) => (value === null ? "—" : `${value} h`);

function Stat({ label, value }: { label: string; value: number | null }) {
  return (
    <div className="hltb-stat">
      <dt>{label}</dt>
      <dd>{hours(value)}</dd>
    </div>
  );
}

function Lookup({ title }: { title: string }) {
  const times = useBackendQuery<Times | null>("lookup", { title });
  if (times.isPending) return <p className="hltb-note">Looking up {title}…</p>;
  if (times.isError)
    return (
      <p role="alert" className="hltb-note">
        HowLongToBeat couldn't be reached: {times.error.message}
      </p>
    );
  if (!times.data) return <p className="hltb-note">HowLongToBeat has no times for {title}.</p>;
  return (
    <section className="hltb" aria-label={`How long to beat ${times.data.title}`}>
      <dl>
        <Stat label="Main story" value={times.data.mainHours} />
        <Stat label="Main + extras" value={times.data.extrasHours} />
        <Stat label="Completionist" value={times.data.completionistHours} />
      </dl>
      <p className="hltb-note">From howlongtobeat.com: {times.data.title}</p>
    </section>
  );
}

function Tab({ gameId }: { gameId: string }) {
  const game = useGame(gameId);
  if (!game.data) return <p className="hltb-note">Loading…</p>;
  return <Lookup title={game.data.title} />;
}

export default definePlugin({
  activate(ctx) {
    ctx.slots.register("game.details.tabs", {
      id: "hltb",
      title: "How long to beat",
      component: Tab,
    });
  },
});
