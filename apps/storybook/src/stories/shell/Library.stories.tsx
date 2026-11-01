import { type CardGame, GameCard, Hero, Shelf } from "@caskade/ui";
import type { Meta } from "@storybook/react-vite";
import { Clock, Gamepad2, ShoppingBag } from "lucide-react";
import { games, hero } from "../../catalogue/fixtures";
import type { Story } from "../../catalogue/stories";

/** The shell's game pieces put together the way Home and Library use them. */
const meta = { title: "Shell/Library", parameters: { layout: "fullscreen" } } satisfies Meta;
export default meta;

const many = Array.from({ length: 12 }, (_, index) => {
  const game = games[index % games.length] as CardGame;
  return { ...game, title: `${game.title} ${Math.floor(index / games.length) + 1}` };
});

export const GameCardGrid: Story = {
  render: () => (
    <div className="grid grid-cols-[repeat(auto-fill,minmax(11rem,1fr))] gap-4 p-6">
      {many.map((game, index) => (
        <GameCard key={game.title} game={game} index={index} />
      ))}
    </div>
  ),
};

export const StoreGrid: Story = {
  render: () => (
    <div className="grid grid-cols-[repeat(auto-fill,minmax(11rem,1fr))] gap-4 p-6">
      {games.map((game, index) => (
        <GameCard key={game.title} game={game} variant="store" index={index} />
      ))}
    </div>
  ),
};

export const HeroBanner: Story = {
  render: () => (
    <div className="p-6">
      <Hero game={hero} eyebrow="Continue playing" action="Play" />
    </div>
  ),
};

export const Home: Story = {
  render: () => (
    <div className="p-6">
      <Hero game={hero} eyebrow="Continue playing" action="Play" />
      <Shelf title="Recently played" icon={Clock} subtitle="This week">
        {games.map((game, index) => (
          <GameCard key={game.title} game={game} variant="recent" index={index} />
        ))}
      </Shelf>
      <Shelf title="Installed" icon={Gamepad2}>
        {games.map((game, index) => (
          <GameCard key={game.title} game={game} index={index} />
        ))}
      </Shelf>
      <Shelf title="Deals" icon={ShoppingBag}>
        {games.map((game, index) => (
          <GameCard key={game.title} game={game} variant="store" index={index} />
        ))}
      </Shelf>
    </div>
  ),
};
