import { useLingui } from "@lingui/react/macro";
import { Link } from "@tanstack/react-router";
import { Fragment } from "react";
import { PluginNav } from "../plugins/PluginViews";
import { useAppWindow } from "./appWindow";
import { type NavItem, primaryNav, secondaryNav } from "./nav";

const itemClass =
  "relative flex items-center gap-3 rounded-control px-3 py-2.5 text-sm text-fg-muted transition-[color,background-color,translate] duration-(--cs-motion-duration-fast) hover:translate-x-0.5 hover:bg-surface-hover hover:text-fg data-[status=active]:text-fg";

function NavLink({ item }: { item: NavItem }) {
  const { t } = useLingui();
  const Icon = item.icon;
  return (
    <Link {...item.link} data-part="nav-item" className={itemClass}>
      <Icon data-part="icon" className="size-5 shrink-0" />
      <span data-part="label" className="flex-1">
        {t(item.label)}
      </span>
    </Link>
  );
}

export function Sidebar() {
  // The brand shares the title bar's row and moves the window like it; on
  // macOS the traffic lights take that row and the brand sits below.
  const overlay = useAppWindow()?.frame === "overlay";
  return (
    <aside
      data-cs="sidebar"
      data-focus-section="sidebar"
      className="relative z-10 flex w-sidebar shrink-0 flex-col border-r border-border bg-surface-raised/80 px-4 pb-6"
    >
      <div
        data-part="brand"
        data-window-drag
        className={`-mx-4 mb-4 flex shrink-0 items-center gap-3 px-6 ${overlay ? "pt-topbar" : "h-topbar"}`}
      >
        <img src="/caskade.svg" alt="" className="size-9" />
        <span className="font-display text-xl font-semibold tracking-[0.3em]">CASKADE</span>
      </div>

      <nav data-part="nav" className="flex flex-col gap-1">
        {primaryNav.map((item) => (
          <Fragment key={item.link.to}>
            <NavLink item={item} />
            {item.sub && <item.sub />}
          </Fragment>
        ))}
      </nav>

      <PluginNav
        className="flex flex-col gap-1"
        itemClass={itemClass}
        iconClass="size-5 shrink-0"
      />

      <hr data-part="divider" className="my-4 border-border" />

      <nav data-part="nav" className="flex flex-col gap-1">
        {secondaryNav.map((item) => (
          <NavLink key={item.link.to} item={item} />
        ))}
      </nav>
    </aside>
  );
}
