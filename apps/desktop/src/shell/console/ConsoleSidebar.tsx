/** Console mode's navigation (#148, concept art `console-mode.png`): the
 * desktop sidebar's links (`../nav.ts`) as big targets, the active rail, and
 * the way back to the desktop. */
import { Trans, useLingui } from "@lingui/react/macro";
import { Link } from "@tanstack/react-router";
import { Monitor } from "lucide-react";
import { Fragment } from "react";
import { Button } from "react-aria-components";
import { PluginNav } from "../../plugins/PluginViews";
import { type NavItem, primaryNav, secondaryNav } from "../nav";
import { useMode } from "./mode";

const itemClass =
  "relative flex items-center gap-4 rounded-control px-4 py-3 text-lg text-fg-muted transition-colors duration-(--cs-motion-duration-fast) hover:bg-surface-hover hover:text-fg focus-visible:bg-surface-hover focus-visible:text-fg data-[status=active]:bg-accent/20 data-[status=active]:text-fg";

function NavLink({ item }: { item: NavItem }) {
  const { t } = useLingui();
  const Icon = item.icon;
  return (
    <Link {...item.link} data-part="nav-item" className={itemClass}>
      <Icon data-part="icon" className="size-6 shrink-0" />
      <span data-part="label" className="flex-1">
        {t(item.label)}
      </span>
    </Link>
  );
}

export function ConsoleSidebar() {
  const { t } = useLingui();
  const { setMode } = useMode();
  return (
    <aside
      data-cs="console-sidebar"
      data-focus-section="sidebar"
      // Scrolls: with the Library's filters open, a 1280×800 screen (a Steam
      // Deck) is shorter than the list, and a pad's focus has to reach the end.
      className="relative z-10 flex w-sidebar shrink-0 flex-col overflow-y-auto border-r border-border bg-surface-raised/80 px-4 pb-6"
    >
      <div data-part="brand" className="flex h-topbar shrink-0 items-center gap-3 px-3">
        <img src="/caskade.svg" alt="" className="size-10" />
        <span className="font-display text-xl font-semibold tracking-[0.3em]">CASKADE</span>
      </div>
      <nav
        data-part="nav"
        aria-label={t({ message: "Console", comment: "The console-mode sidebar's navigation" })}
        className="mt-4 flex flex-col gap-1.5"
      >
        {[...primaryNav, ...secondaryNav].map((item) => (
          <Fragment key={item.link.to}>
            <NavLink item={item} />
            {item.sub && <item.sub console />}
          </Fragment>
        ))}
      </nav>
      <PluginNav
        className="mt-4 flex flex-col gap-1.5"
        itemClass={itemClass}
        iconClass="size-6 shrink-0"
      />
      <Button
        data-part="leave"
        onPress={() => setMode("desktop")}
        className="mt-auto flex items-center gap-4 rounded-control px-4 py-3 text-fg-muted data-focus-visible:bg-surface-hover data-hovered:bg-surface-hover data-focus-visible:text-fg data-hovered:text-fg"
      >
        <Monitor className="size-6 shrink-0" />
        <Trans>Desktop mode</Trans>
      </Button>
    </aside>
  );
}
