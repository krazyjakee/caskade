import type { LucideIcon } from "lucide-react";
import {
  ToggleButton,
  ToggleButtonGroup,
  type ToggleButtonGroupProps,
} from "react-aria-components";

export interface SegmentedOption<K extends string> {
  id: K;
  label: string;
  icon?: LucideIcon;
}

export interface SegmentedControlProps<K extends string>
  extends Omit<
    ToggleButtonGroupProps,
    | "selectionMode"
    | "selectedKeys"
    | "defaultSelectedKeys"
    | "onSelectionChange"
    | "children"
    | "className"
  > {
  options: readonly SegmentedOption<K>[];
  value: K;
  onChange: (value: K) => void;
  className?: string;
}

/** Single choice from a few options, shown side by side. */
export function SegmentedControl<K extends string>({
  options,
  value,
  onChange,
  className = "",
  ...props
}: SegmentedControlProps<K>) {
  return (
    <ToggleButtonGroup
      {...props}
      data-cs="segmented"
      selectionMode="single"
      disallowEmptySelection
      selectedKeys={[value]}
      onSelectionChange={(keys) => {
        const option = options.find((o) => keys.has(o.id));
        if (option) onChange(option.id);
      }}
      className={`inline-flex rounded-control border border-border-strong bg-surface-base/60 p-0.5 data-disabled:opacity-40 ${className}`}
    >
      {options.map(({ id, label, icon: Icon }) => (
        <ToggleButton
          key={id}
          id={id}
          data-part="segment"
          className="flex items-center gap-1.5 rounded-[6px] px-3 py-1.5 text-sm whitespace-nowrap text-fg-muted transition-[background-color,color,box-shadow] duration-(--cs-motion-duration-fast) data-focus-visible:focus-ring data-hovered:text-fg data-selected:bg-accent/25 data-selected:text-fg data-selected:shadow-[inset_0_0_0_1px] data-selected:shadow-accent-alt/50"
        >
          {Icon && <Icon data-part="icon" className="size-4" />}
          {label}
        </ToggleButton>
      ))}
    </ToggleButtonGroup>
  );
}
