import type { LucideIcon } from "lucide-react";
import {
  type KeyboardEvent,
  type MouseEvent,
  type ReactNode,
  type Ref,
  useEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import {
  Menu as AriaMenu,
  MenuItem as AriaMenuItem,
  type MenuItemProps as AriaMenuItemProps,
  type MenuProps as AriaMenuProps,
  MenuSection as AriaMenuSection,
  type MenuSectionProps as AriaMenuSectionProps,
  Header,
  Popover,
  type PopoverProps,
  Separator,
  Text,
} from "react-aria-components";
import { createPortal } from "react-dom";

/** Every menu's popover: a button's dropdown (inside a React Aria
 * `MenuTrigger`) and a `ContextMenu` alike. */
export function MenuPopover({
  className = "",
  ...props
}: Omit<PopoverProps, "className"> & { className?: string; ref?: Ref<HTMLElement> }) {
  return (
    <Popover
      offset={6}
      {...props}
      data-cs="menu"
      className={`z-40 min-w-56 max-w-80 rounded-control border border-border-strong bg-surface-overlay p-1 shadow-xl shadow-black/40 outline-none data-entering:animate-enter ${className}`}
    />
  );
}

export type MenuProps<T> = Omit<AriaMenuProps<T>, "className"> & { className?: string };

export function Menu<T extends object>({ className = "", ...props }: MenuProps<T>) {
  return (
    <AriaMenu
      {...props}
      data-part="list"
      className={`max-h-[70vh] overflow-y-auto outline-none ${className}`}
    />
  );
}

export interface MenuItemProps extends Omit<AriaMenuItemProps, "className" | "children"> {
  /** The item's label. */
  children: ReactNode;
  icon?: LucideIcon;
  /** A second line under the label. */
  description?: ReactNode;
  /** `danger` for actions that remove or destroy something. */
  tone?: "danger";
  className?: string;
}

export function MenuItem({
  children,
  icon: Icon,
  description,
  tone,
  textValue,
  className = "",
  ...props
}: MenuItemProps) {
  return (
    <AriaMenuItem
      {...props}
      data-part="item"
      data-tone={tone}
      textValue={textValue ?? (typeof children === "string" ? children : undefined)}
      className={`flex cursor-default items-start gap-2.5 rounded-[6px] px-2.5 py-1.5 text-sm text-fg-muted outline-none console:data-focus-visible:focus-ring data-focused:bg-surface-hover data-focused:text-fg data-disabled:opacity-40 data-[tone=danger]:text-danger ${className}`}
    >
      {Icon && <Icon data-part="icon" aria-hidden="true" className="mt-0.5 size-4 shrink-0" />}
      <span className="flex min-w-0 flex-col">
        <Text slot="label" data-part="label">
          {children}
        </Text>
        {description && (
          <Text slot="description" data-part="description" className="text-xs text-fg-subtle">
            {description}
          </Text>
        )}
      </span>
    </AriaMenuItem>
  );
}

export function MenuSection<T extends object>({
  title,
  children,
  ...props
}: Omit<AriaMenuSectionProps<T>, "className" | "children"> & {
  /** A heading over the section's items. */
  title?: string;
  children: ReactNode;
}) {
  return (
    <AriaMenuSection {...props} data-part="section">
      {title && (
        <Header
          data-part="section-title"
          className="px-2.5 pt-2 pb-1 font-mono text-[10px] tracking-wider text-fg-subtle uppercase"
        >
          {title}
        </Header>
      )}
      {children}
    </AriaMenuSection>
  );
}

export function MenuSeparator() {
  return <Separator data-part="separator" className="mx-1 my-1 border-t border-border" />;
}

/** Where a context menu opens: at the pointer, or beside an element (the
 * context menu key, Shift+F10, a pad's Start, an actions button). */
export type MenuAnchor = { x: number; y: number } | Element;

export interface ContextMenuProps<T> extends Omit<MenuProps<T>, "onClose"> {
  anchor: MenuAnchor;
  onClose: () => void;
}

/** A menu that opens at `anchor` instead of under a trigger: a right click's
 * menu. Mount it while it is open; `useContextMenu` keeps the anchor. A right
 * click anywhere else closes it, so that target's own menu can open. */
export function ContextMenu<T extends object>(props: ContextMenuProps<T>) {
  // A new anchor (another right click) opens a new menu there.
  const [shown, setShown] = useState({ anchor: props.anchor, count: 0 });
  if (shown.anchor !== props.anchor) setShown({ anchor: props.anchor, count: shown.count + 1 });
  return <AnchoredMenu key={shown.count} {...props} />;
}

function AnchoredMenu<T extends object>({ anchor, onClose, ...props }: ContextMenuProps<T>) {
  const point = anchor instanceof Element ? null : anchor;
  const pointRef = useRef<HTMLSpanElement>(null);
  const elementRef = useMemo(
    () => ({ current: anchor instanceof Element ? anchor : null }),
    [anchor],
  );
  const popover = useRef<HTMLElement>(null);
  const close = useRef(onClose);
  close.current = onClose;
  useEffect(() => {
    const elsewhere = (event: Event) => {
      if (!(event.target instanceof Node && popover.current?.contains(event.target)))
        close.current();
    };
    window.addEventListener("contextmenu", elsewhere, { capture: true });
    return () => window.removeEventListener("contextmenu", elsewhere, { capture: true });
  }, []);
  return (
    <>
      {point &&
        createPortal(
          // A fixed point at the body, out of reach of the target's transforms
          // and containment, for the popover to hang from.
          <span
            ref={pointRef}
            aria-hidden="true"
            style={{ position: "fixed", left: point.x, top: point.y, width: 0, height: 0 }}
          />,
          document.body,
        )}
      <MenuPopover
        ref={popover}
        isOpen
        onOpenChange={(open) => {
          if (!open) onClose();
        }}
        triggerRef={point ? pointRef : elementRef}
        placement={point ? "bottom start" : "bottom end"}
        offset={point ? 2 : 6}
      >
        <Menu autoFocus="first" {...props} onClose={onClose} />
      </MenuPopover>
    </>
  );
}

/** The handlers that open a context menu from the element they are spread
 * on: a right click opens it at the pointer, the context menu key and
 * Shift+F10 beside the element. */
export function contextMenuTarget(open: (anchor: MenuAnchor) => void) {
  return {
    onContextMenu: (event: MouseEvent) => {
      event.preventDefault();
      // The innermost target's menu wins.
      event.stopPropagation();
      // The context menu key's event has no pointer position.
      open(
        event.clientX === 0 && event.clientY === 0
          ? event.currentTarget
          : { x: event.clientX, y: event.clientY },
      );
    },
    onKeyDown: (event: KeyboardEvent) => {
      if (event.key === "ContextMenu" || (event.shiftKey && event.key === "F10")) {
        event.preventDefault();
        open(event.currentTarget);
      }
    },
  };
}

/** Opens a `ContextMenu` from its target: spread `targetProps` on the element
 * that is right-clicked, and call `open` for other ways in (a pad's Start, an
 * actions button). */
export function useContextMenu() {
  const [anchor, setAnchor] = useState<MenuAnchor | null>(null);
  return {
    anchor,
    open: setAnchor as (anchor: MenuAnchor) => void,
    close: () => setAnchor(null),
    targetProps: contextMenuTarget(setAnchor),
  };
}
