/** The sidebar's links, the same in desktop and console mode. */
import type { MessageDescriptor } from "@lingui/core";
import { msg } from "@lingui/core/macro";
import type { LinkProps } from "@tanstack/react-router";
import { House, LayoutGrid, type LucideIcon, Settings, SquareLibrary, Store } from "lucide-react";
import type { ComponentType } from "react";
import { LibraryNav } from "./LibraryNav";

export interface NavItem {
  /** Translated where it is shown (`t(item.label)`). */
  label: MessageDescriptor;
  icon: LucideIcon;
  link: LinkProps;
  /** Links under this one (the Library's filters). */
  sub?: ComponentType<{ console?: boolean }>;
}

export const primaryNav: NavItem[] = [
  {
    label: msg({
      message: "Home",
      context: "navigation",
      comment: "The sidebar link to the home view",
    }),
    icon: House,
    link: { to: "/", activeOptions: { exact: true } },
  },
  {
    label: msg`Library`,
    icon: LayoutGrid,
    // The library as it was last left (its remembered filter, sort and layout).
    link: { to: "/library", activeOptions: { includeSearch: false } },
    sub: LibraryNav,
  },
  {
    label: msg`Store`,
    icon: Store,
    link: { to: "/store", activeOptions: { includeSearch: false } },
  },
  {
    label: msg`Collections`,
    icon: SquareLibrary,
    link: { to: "/collections", activeOptions: { includeSearch: false } },
  },
];

/** The links below the divider. */
export const secondaryNav: NavItem[] = [
  {
    label: msg`Settings`,
    icon: Settings,
    link: { to: "/settings", activeOptions: { includeSearch: false } },
  },
];
