import { Check, ChevronDown } from "lucide-react";
import {
  Select as AriaSelect,
  type SelectProps as AriaSelectProps,
  Button,
  Label,
  ListBox,
  ListBoxItem,
  Popover,
  SelectValue,
} from "react-aria-components";

export interface SelectOption<K extends string> {
  id: K;
  label: string;
}

export interface SelectProps<K extends string>
  extends Omit<AriaSelectProps, "value" | "onChange" | "children" | "className"> {
  label?: string;
  options: readonly SelectOption<K>[];
  value: K;
  onChange: (value: K) => void;
  className?: string;
}

export function Select<K extends string>({
  label,
  options,
  value,
  onChange,
  className = "",
  ...props
}: SelectProps<K>) {
  return (
    <AriaSelect
      {...props}
      data-cs="select"
      value={value}
      onChange={(key) => {
        const option = options.find((o) => o.id === key);
        if (option) onChange(option.id);
      }}
      className={`group flex flex-col gap-1.5 data-disabled:opacity-40 ${className}`}
    >
      {label && (
        <Label data-part="label" className="text-sm font-medium">
          {label}
        </Label>
      )}
      <Button
        data-part="trigger"
        className="flex h-9 min-w-32 items-center justify-between gap-2 rounded-control border border-border-strong bg-surface-overlay/70 px-3 text-left text-sm backdrop-blur transition-[border-color,box-shadow] duration-(--cs-motion-duration-fast) data-focus-visible:focus-ring data-hovered:border-accent-alt/50 group-data-open:border-accent-alt/60 group-data-open:shadow-[0_0_16px_-6px] group-data-open:shadow-accent-alt"
      >
        <SelectValue data-part="value" className="truncate" />
        <ChevronDown
          data-part="chevron"
          className="size-4 shrink-0 text-fg-muted transition-transform duration-(--cs-motion-duration-base) group-data-open:rotate-180"
        />
      </Button>
      <Popover
        data-cs="select-popover"
        offset={6}
        className="min-w-(--trigger-width) rounded-control border border-border-strong bg-surface-overlay p-1 shadow-xl shadow-black/40 data-entering:animate-enter"
      >
        <ListBox className="max-h-72 overflow-y-auto outline-none">
          {options.map((option) => (
            <ListBoxItem
              key={option.id}
              id={option.id}
              textValue={option.label}
              data-part="option"
              className="flex cursor-default items-center justify-between gap-4 rounded-[6px] px-2.5 py-1.5 text-sm text-fg-muted outline-none console:data-focus-visible:focus-ring data-focused:bg-surface-hover data-focused:text-fg data-selected:text-fg"
            >
              {({ isSelected }) => (
                <>
                  {option.label}
                  {isSelected && <Check data-part="check" className="size-4 text-accent-alt" />}
                </>
              )}
            </ListBoxItem>
          ))}
        </ListBox>
      </Popover>
    </AriaSelect>
  );
}
