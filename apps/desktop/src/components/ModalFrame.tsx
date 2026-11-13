import type { ReactNode } from "react";
import { Modal, ModalOverlay } from "react-aria-components";

/** The app's modal: a dimmed, blurred overlay holding a panel that
 * `className` sizes and styles. Escape or a click outside calls `close`,
 * except while `busy`. */
export function ModalFrame({
  busy = false,
  close,
  consoleFill = false,
  className,
  children,
}: {
  busy?: boolean;
  close: () => void;
  /** The overlay drops its padding in console mode, for a panel that fills
   * the screen there. */
  consoleFill?: boolean;
  className: string;
  children: ReactNode;
}) {
  return (
    <ModalOverlay
      isOpen
      isDismissable={!busy}
      onOpenChange={(open) => {
        if (!open && !busy) close();
      }}
      className={`fixed inset-0 z-50 grid place-items-center bg-surface-base/80 p-4 backdrop-blur-sm${consoleFill ? " console:p-0" : ""}`}
    >
      <Modal className={className}>{children}</Modal>
    </ModalOverlay>
  );
}
