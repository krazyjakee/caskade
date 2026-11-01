import { Check, Minus } from "lucide-react";
import type { ReactNode } from "react";
import {
  Checkbox as AriaCheckbox,
  type CheckboxProps as AriaCheckboxProps,
} from "react-aria-components";

export interface CheckboxProps extends Omit<AriaCheckboxProps, "children" | "className"> {
  className?: string;
  children?: ReactNode;
}

export function Checkbox({ className = "", children, ...props }: CheckboxProps) {
  return (
    <AriaCheckbox
      {...props}
      data-cs="checkbox"
      className={`group flex items-center gap-3 text-sm data-disabled:opacity-40 ${className}`}
    >
      <span
        data-part="box"
        className="grid size-4.5 shrink-0 place-items-center rounded-[5px] border border-border-strong bg-surface-base/60 transition-[background-color,border-color] duration-(--cs-motion-duration-fast) group-data-hovered:border-accent-alt/50 group-data-focus-visible:focus-ring group-data-indeterminate:border-accent-alt/60 group-data-indeterminate:bg-accent group-data-selected:border-accent-alt/60 group-data-selected:bg-accent"
      >
        <Check
          data-part="mark"
          strokeWidth={3}
          className="size-3 text-accent-fg opacity-0 transition-opacity duration-(--cs-motion-duration-fast) group-data-selected:opacity-100 group-data-indeterminate:hidden"
        />
        <Minus
          data-part="mark"
          strokeWidth={3}
          className="hidden size-3 text-accent-fg group-data-indeterminate:block"
        />
      </span>
      {children !== undefined && <span data-part="label">{children}</span>}
    </AriaCheckbox>
  );
}
