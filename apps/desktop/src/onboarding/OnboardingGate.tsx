import { type ReactNode, useEffect, useState } from "react";
import { useSettingsSection } from "../views/settings/useSettings";
import { hideBootSplash } from "./boot";
import { Onboarding, type OnboardingSettings } from "./Onboarding";

/** First-run setup in place of the app while `core.onboarding.completed`
 * is false, and again when Settings asks for it. Without a core (the
 * browser preview) the app shows straight away. */
export function OnboardingGate({ children, onDone }: { children: ReactNode; onDone?(): void }) {
  const prefs = useSettingsSection<OnboardingSettings>("core.onboarding");
  const [open, setOpen] = useState<boolean | null>(null);
  const completed = prefs.settings?.completed;
  const failed = Boolean(prefs.error);
  useEffect(() => {
    // Finishing doesn't close it here: setup marks itself done before its
    // last screen, and closes with `onDone`.
    if (completed === false) setOpen(true);
    else if (completed === true || failed) setOpen((current) => current ?? false);
  }, [completed, failed]);
  useEffect(() => {
    if (open !== null) hideBootSplash();
  }, [open]);
  if (open === null) return null;
  if (!open) return children;
  return (
    <Onboarding
      onDone={() => {
        setOpen(false);
        onDone?.();
      }}
    />
  );
}
