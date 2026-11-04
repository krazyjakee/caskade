import type { LucideIcon } from "lucide-react";
import type { ReactNode } from "react";
import {
  TextField as AriaTextField,
  type TextFieldProps as AriaTextFieldProps,
  FieldError,
  Input,
  Label,
  Text,
} from "react-aria-components";

export interface TextFieldProps extends Omit<AriaTextFieldProps, "children" | "className"> {
  label?: string;
  description?: string;
  placeholder?: string;
  icon?: LucideIcon;
  /** Monospace input, for paths and identifiers. */
  mono?: boolean;
  /** Control beside the input, e.g. a "Browse…" button. */
  action?: ReactNode;
  className?: string;
}

export function TextField({
  label,
  description,
  placeholder,
  icon: Icon,
  mono = false,
  action,
  className = "",
  ...props
}: TextFieldProps) {
  return (
    <AriaTextField
      {...props}
      data-cs="text-field"
      className={`group flex flex-col gap-1.5 data-disabled:opacity-40 ${className}`}
    >
      {label && (
        <Label data-part="label" className="text-sm font-medium">
          {label}
        </Label>
      )}
      <div className="flex gap-2">
        <div
          data-part="control"
          className="flex h-9 flex-1 items-center gap-2 rounded-control border border-border-strong bg-surface-base/60 px-3 transition-[border-color,box-shadow] duration-(--cs-motion-duration-fast) focus-within:border-accent-alt/60 focus-within:shadow-[0_0_16px_-6px] focus-within:shadow-accent-alt console:focus-within:focus-ring group-data-invalid:border-danger/70"
        >
          {Icon && <Icon data-part="icon" className="size-4 shrink-0 text-fg-subtle" />}
          <Input
            data-part="input"
            placeholder={placeholder}
            className={`w-full min-w-0 bg-transparent placeholder:text-fg-subtle focus:outline-none ${mono ? "font-mono text-xs" : "text-sm"}`}
          />
        </div>
        {action && (
          <div data-part="action" className="flex shrink-0">
            {action}
          </div>
        )}
      </div>
      {description && (
        <Text slot="description" data-part="description" className="text-xs text-fg-muted">
          {description}
        </Text>
      )}
      <FieldError data-part="error" className="text-xs text-danger" />
    </AriaTextField>
  );
}
