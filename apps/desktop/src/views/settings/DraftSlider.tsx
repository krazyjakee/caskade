import { Slider, type SliderProps } from "@caskade/ui";
import { useEffect, useRef, useState } from "react";

/** A slider that saves once it is let go (#55): dragging moves only the
 * slider, so a setting that changes the page (the interface scale) doesn't
 * rescale it under the pointer, and a drag is one save, not one per step.
 * Keys and a pad's presses end each step, so each saves. */
export function DraftSlider({
  value,
  commit,
  ...props
}: Omit<SliderProps, "value" | "defaultValue" | "onChange" | "onChangeEnd"> & {
  value: number;
  commit: (next: number) => void;
}) {
  const [draft, setDraft] = useState(value);
  const dragging = useRef(false);
  useEffect(() => {
    if (!dragging.current) setDraft(value);
  }, [value]);
  return (
    <Slider
      {...props}
      value={draft}
      onChange={(next) => {
        dragging.current = true;
        setDraft(next);
      }}
      onChangeEnd={(next) => {
        dragging.current = false;
        if (next !== value) commit(next);
      }}
    />
  );
}
