import type { ReactNode } from "react";
import { Switch as AriaSwitch, type SwitchProps as AriaSwitchProps } from "react-aria-components";

const sizes = {
  sm: { track: "h-5 w-9", thumb: "size-3.5 group-data-selected:translate-x-4" },
  md: { track: "h-6 w-11", thumb: "size-4.5 group-data-selected:translate-x-5" },
};

export interface SwitchProps extends Omit<AriaSwitchProps, "children" | "className"> {
  size?: keyof typeof sizes;
  className?: string;
  children?: ReactNode;
}

export function Switch({ size = "md", className = "", children, ...props }: SwitchProps) {
  return (
    <AriaSwitch
      {...props}
      data-cs="switch"
      data-size={size}
      className={`group inline-flex items-center gap-2.5 text-sm data-disabled:opacity-40 ${className}`}
    >
      <span
        data-part="track"
        className={`flex shrink-0 items-center rounded-full border border-border-strong bg-surface-hover px-0.5 transition-[background-color,border-color,box-shadow] duration-(--cs-motion-duration-base) group-data-focus-visible:focus-ring group-data-selected:border-accent-alt/60 group-data-selected:bg-accent group-data-selected:shadow-[0_0_14px_-3px] group-data-selected:shadow-accent-alt ${sizes[size].track}`}
      >
        <span
          data-part="thumb"
          className={`rounded-full bg-fg-muted shadow-sm transition-[translate,background-color] duration-(--cs-motion-duration-base) ease-(--cs-motion-ease-out) group-data-selected:bg-white ${sizes[size].thumb}`}
        />
      </span>
      {children !== undefined && <span data-part="label">{children}</span>}
    </AriaSwitch>
  );
}
