import type { LucideIcon } from "lucide-react";
import { type ReactNode, useId } from "react";
import {
  Provider,
  SelectContext,
  SliderContext,
  SwitchContext,
  TextFieldContext,
  ToggleButtonGroupContext,
} from "react-aria-components";

export function SettingsSection({
  icon: Icon,
  title,
  description,
  aside,
  className = "",
  children,
}: {
  icon?: LucideIcon;
  title: string;
  description?: string;
  /** Badge or actions at the end of the header. */
  aside?: ReactNode;
  className?: string;
  children: ReactNode;
}) {
  const headingId = useId();
  return (
    <section
      data-cs="settings-section"
      aria-labelledby={headingId}
      className={`rounded-panel border border-border bg-surface-raised p-5 ${className}`}
    >
      <header data-part="header" className="mb-2 flex items-start gap-3">
        {Icon && <Icon data-part="icon" className="mt-0.5 size-5 shrink-0 text-accent-alt" />}
        <div className="min-w-0 flex-1">
          <h2
            id={headingId}
            data-part="title"
            className="font-display text-lg font-semibold tracking-wide"
          >
            {title}
          </h2>
          {description && (
            <p data-part="description" className="text-sm text-fg-subtle">
              {description}
            </p>
          )}
        </div>
        {aside && (
          <div data-part="aside" className="flex shrink-0 items-center gap-3">
            {aside}
          </div>
        )}
      </header>
      {children}
    </section>
  );
}

/**
 * One setting: label and description, then its controls. The text grows to a
 * fixed width and no further, so on a wide panel the controls stay beside their
 * label, in a column shared by the rows above and below. Controls inside are labelled by the row through React Aria contexts, so they
 * need no label of their own.
 */
export function SettingRow({
  icon: Icon,
  label,
  description,
  children,
}: {
  icon?: LucideIcon;
  label: string;
  description?: string;
  children: ReactNode;
}) {
  const labelId = useId();
  const descriptionId = useId();
  const labelling = {
    "aria-labelledby": labelId,
    "aria-describedby": description ? descriptionId : undefined,
  };
  return (
    <div
      data-cs="setting-row"
      className="flex items-center gap-3 border-t border-border py-3 first-of-type:border-t-0"
    >
      {Icon && <Icon data-part="icon" className="size-5 shrink-0 text-fg-muted" />}
      <div className="max-w-md min-w-0 flex-1">
        <div id={labelId} data-part="label" className="text-sm font-medium">
          {label}
        </div>
        {description && (
          <div id={descriptionId} data-part="description" className="text-xs text-fg-subtle">
            {description}
          </div>
        )}
      </div>
      <Provider
        values={[
          [SwitchContext, labelling],
          [SelectContext, labelling],
          [TextFieldContext, labelling],
          // The slider thumb is labelled by its group, and a browser only follows one
          // aria-labelledby hop, so the group needs a real aria-label.
          [SliderContext, { ...labelling, "aria-labelledby": undefined, "aria-label": label }],
          [ToggleButtonGroupContext, labelling],
        ]}
      >
        <div data-part="controls" className="flex shrink-0 items-center gap-3">
          {children}
        </div>
      </Provider>
    </div>
  );
}

const tones = {
  ok: "bg-status-installed shadow-status-installed",
  off: "bg-status-missing shadow-transparent",
  warning: "bg-warning shadow-warning",
  danger: "bg-danger shadow-danger",
};

/** Small coloured dot and label, e.g. "Detected" / "Not found". */
export function StatusDot({ tone, children }: { tone: keyof typeof tones; children: ReactNode }) {
  return (
    <span
      data-cs="status-dot"
      data-tone={tone}
      className="inline-flex items-center gap-1.5 text-xs whitespace-nowrap text-fg-muted"
    >
      <span data-part="dot" className={`size-1.5 rounded-full shadow-[0_0_6px] ${tones[tone]}`} />
      <span data-part="label">{children}</span>
    </span>
  );
}

const pillTones = {
  warning: "border-warning/40 bg-warning/10 text-warning",
  danger: "border-danger/40 bg-danger/10 text-danger",
  accent: "border-accent-alt/40 bg-accent-alt/10 text-accent-alt",
};

/** Bordered chip with an icon, e.g. "Off by default" / "Caution". */
export function Pill({
  tone,
  icon: Icon,
  children,
}: {
  tone: keyof typeof pillTones;
  icon: LucideIcon;
  children: ReactNode;
}) {
  return (
    <span
      data-cs="pill"
      data-tone={tone}
      className={`inline-flex shrink-0 items-center gap-1.5 rounded-control border px-2.5 py-1 text-xs font-medium whitespace-nowrap ${pillTones[tone]}`}
    >
      <Icon data-part="icon" className="size-3.5" />
      <span data-part="label">{children}</span>
    </span>
  );
}
