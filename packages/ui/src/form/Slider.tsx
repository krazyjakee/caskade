import {
  Slider as AriaSlider,
  type SliderProps as AriaSliderProps,
  Label,
  SliderOutput,
  SliderThumb,
  SliderTrack,
} from "react-aria-components";

export interface SliderProps extends Omit<AriaSliderProps<number>, "children" | "className"> {
  label?: string;
  /** Formats the value readout, e.g. `(v) => `${v}px``. */
  format?: (value: number) => string;
  className?: string;
}

export function Slider({ label, format = String, className = "", ...props }: SliderProps) {
  const output = (
    <SliderOutput
      data-part="output"
      className="min-w-10 text-right font-mono text-xs text-fg-muted"
    >
      {({ state }) => format(state.getThumbValue(0))}
    </SliderOutput>
  );
  return (
    <AriaSlider
      {...props}
      data-cs="slider"
      className={`group flex min-w-48 flex-col gap-1 data-disabled:opacity-40 ${className}`}
    >
      {label ? (
        <div className="flex items-center justify-between gap-3">
          <Label data-part="label" className="text-sm font-medium">
            {label}
          </Label>
          {output}
        </div>
      ) : null}
      <div className="flex items-center gap-3">
        <SliderTrack data-part="track" className="relative h-5 flex-1">
          {({ state }) => (
            <>
              <div className="absolute top-1/2 h-1.5 w-full -translate-y-1/2 rounded-full bg-surface-hover" />
              <div
                data-part="fill"
                className="absolute top-1/2 h-1.5 -translate-y-1/2 rounded-full bg-linear-to-r from-accent to-accent-alt"
                style={{ width: `${state.getThumbPercent(0) * 100}%` }}
              />
              <SliderThumb
                data-part="thumb"
                className="top-1/2 size-4 rounded-full border-2 border-accent-alt bg-white shadow-[0_0_10px] shadow-accent-alt/60 transition-[scale] duration-(--cs-motion-duration-fast) data-dragging:scale-115 data-focus-visible:focus-ring-outside"
              />
            </>
          )}
        </SliderTrack>
        {/* Without a label the readout sits beside the track, e.g. inside a SettingRow. */}
        {label ? null : output}
      </div>
    </AriaSlider>
  );
}
