import type { Meta } from "@storybook/react-vite";
import { catalogGame, fixtureCatalog, release } from "../../../../desktop/src/api/catalogFixtures";
import { AgeRatingLabel } from "../../../../desktop/src/components/AgeRatingLabel";
import { CatalogCard } from "../../../../desktop/src/views/store/CatalogCard";
import { WhereToBuy } from "../../../../desktop/src/views/store/WhereToBuy";
import type { Story } from "../../catalogue/stories";

/** The Store's catalog pieces (#163), the app's own components over the
 * fixture catalog (`apps/desktop/src/api/catalogFixtures.ts`): cards with
 * the library's status, release labels as precise as IGDB knows them, and
 * a game's store links and age rating (on a fixed board: the preferred one
 * comes from the app's settings). */
const meta = { title: "Shell/Store", parameters: { layout: "fullscreen" } } satisfies Meta;
export default meta;

const href = (id: string) => `#/store/games/${id}`;
const [lantern, copper, starfall, harbour, ashen] = fixtureCatalog;

export const CatalogGrid: Story = {
  render: () => (
    <div className="grid grid-cols-[repeat(auto-fill,minmax(11rem,1fr))] gap-4 p-6">
      {lantern && (
        <CatalogCard
          game={lantern}
          href={href(lantern.id)}
          owned={{ igdbId: lantern.id, gameId: "lantern", installed: true }}
        />
      )}
      {copper && (
        <CatalogCard
          game={copper}
          href={href(copper.id)}
          owned={{ igdbId: copper.id, gameId: "copper", installed: false }}
          index={1}
        />
      )}
      {fixtureCatalog.slice(5, 9).map((game, i) => (
        <CatalogCard key={game.id} game={game} href={href(game.id)} index={i + 2} />
      ))}
    </div>
  ),
};

/** Upcoming releases: a month, a quarter, a year (cancelled) and TBD. */
export const ReleaseLabels: Story = {
  render: () => (
    <div className="grid grid-cols-[repeat(auto-fill,minmax(11rem,1fr))] gap-4 p-6">
      {starfall && (
        <CatalogCard game={starfall} href={href(starfall.id)} release={starfall.releaseDates[0]} />
      )}
      {lantern && (
        <CatalogCard
          game={lantern}
          href={href(lantern.id)}
          release={lantern.releaseDates[2]}
          index={1}
        />
      )}
      {ashen && (
        <CatalogCard
          game={ashen}
          href={href(ashen.id)}
          release={release("2026-12-01", {
            precision: "year",
            year: 2026,
            human: "2026",
            status: "Cancelled",
          })}
          index={2}
        />
      )}
      {harbour && (
        <CatalogCard
          game={harbour}
          href={href(harbour.id)}
          release={harbour.releaseDates[0]}
          index={3}
        />
      )}
    </div>
  ),
};

export const WhereToBuyLinks: Story = {
  render: () => (
    <div className="flex max-w-md flex-col gap-6 p-6">
      <WhereToBuy links={lantern?.storeLinks ?? []} />
      <WhereToBuy links={catalogGame("1", "Unsold").storeLinks} />
      <p className="flex items-center gap-2 text-sm text-fg-muted">
        Age rating <AgeRatingLabel ratings={lantern?.ageRatings ?? []} board="pegi" />
      </p>
    </div>
  ),
};
