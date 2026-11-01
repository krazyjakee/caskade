import type { LucideIcon } from "lucide-react";
import type { ReactNode, Ref } from "react";
import { Button as AriaButton, type ButtonProps as AriaButtonProps } from "react-aria-components";

const variants = {
  primary:
    "border-transparent bg-linear-to-r from-accent to-accent-strong text-accent-fg data-hovered:brightness-115 data-hovered:shadow-[0_0_18px_-4px] data-hovered:shadow-glow",
  secondary:
    "border-border-strong bg-surface-overlay/70 text-fg backdrop-blur data-hovered:border-accent-alt/50 data-hovered:bg-surface-hover",
  ghost: "border-transparent text-fg-muted data-hovered:bg-surface-hover data-hovered:text-fg",
  danger:
    "border-danger/40 bg-danger/10 text-danger data-hovered:border-danger/70 data-hovered:bg-danger/20",
};

const sizes = {
  sm: "h-8 gap-1.5 px-3 text-xs",
  md: "h-9 gap-2 px-4 text-sm",
};

export interface ButtonProps extends Omit<AriaButtonProps, "children" | "className"> {
  variant?: keyof typeof variants;
  size?: keyof typeof sizes;
  icon?: LucideIcon;
  className?: string;
  children?: ReactNode;
  /** For a view's first stop (`useDefaultFocus`). */
  ref?: Ref<HTMLButtonElement>;
}

export function Button({
  variant = "secondary",
  size = "md",
  icon: Icon,
  className = "",
  children,
  ...props
}: ButtonProps) {
  return (
    <AriaButton
      {...props}
      data-cs="button"
      data-variant={variant}
      data-size={size}
      className={`inline-flex shrink-0 items-center justify-center rounded-control border font-medium whitespace-nowrap transition-[background-color,border-color,box-shadow,filter,scale] duration-(--cs-motion-duration-fast) data-disabled:opacity-40 data-focus-visible:focus-ring data-pressed:scale-[0.97] ${variants[variant]} ${sizes[size]} ${className}`}
    >
      {Icon && <Icon data-part="icon" className={size === "sm" ? "size-3.5" : "size-4"} />}
      {children}
    </AriaButton>
  );
}
