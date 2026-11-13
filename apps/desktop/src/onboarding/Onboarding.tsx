/** First-run setup: a welcome, short questions (the last, the admin's
 * account, when there is one to set up), then the work they start, shown
 * as it happens, with sounds. `OnboardingGate` shows it in
 * place of the app until `core.onboarding.completed` is set. */
import { Button } from "@caskade/ui";
import { Trans, useLingui } from "@lingui/react/macro";
import { Volume2, VolumeX } from "lucide-react";
import { useEffect, useMemo, useRef, useState } from "react";
import { Button as AriaButton } from "react-aria-components";
import { useAccounts, useActiveAccount } from "../api/accounts";
import { useProvidedSyncApi } from "../api/sync";
import { PathPickerProvider } from "../components/pathBrowser/PathPicker";
import { PairingHost } from "../components/sync/PairingHost";
import { InputProvider, useDefaultFocus } from "../input";
import { AccountPicker } from "../shell/accounts/AccountPicker";
import { useAppWindow } from "../shell/appWindow";
import { useMode } from "../shell/console/mode";
import { useWindowDrag } from "../shell/useWindowDrag";
import { WindowControls } from "../shell/WindowControls";
import { useSettingsApi, useSettingsSection } from "../views/settings/useSettings";
import { AccountStep } from "./AccountStep";
import { DeviceStep } from "./DeviceStep";
import { GamesStep } from "./GamesStep";
import { LanguageStep } from "./LanguageStep";
import { FolderStep, ModeStep, SourcesStep } from "./Questions";
import { type Choices, defaultChoices, markCompleted } from "./run";
import { SetupStep } from "./SetupStep";
import { createSounds } from "./sound";

type Step =
  | "welcome"
  | "device"
  | "language"
  | "folder"
  | "games"
  | "sources"
  | "mode"
  | "account"
  | "setup";
const basics: Step[] = ["language", "folder", "sources", "mode"];

export interface OnboardingSettings {
  completed: boolean;
  sounds: boolean;
}

export function Onboarding({ onDone }: { onDone(): void }) {
  const { t } = useLingui();
  const settingsApi = useSettingsApi();
  const prefs = useSettingsSection<OnboardingSettings>("core.onboarding");
  // Local too, so the toggle works when the setting can't be saved.
  const [muted, setMuted] = useState<boolean | null>(null);
  const soundsOn = muted === null ? (prefs.settings?.sounds ?? true) : !muted;
  const enabled = useRef(soundsOn);
  enabled.current = soundsOn;
  const sounds = useMemo(() => createSounds(() => enabled.current), []);
  const [step, setStep] = useState<Step>("welcome");
  // Set up from another device (#242): its library comes over first.
  const [fromDevice, setFromDevice] = useState(false);
  const devices = useProvidedSyncApi();
  const accountList = useAccounts().data;
  const [choices, setChoices] = useState<Choices>(defaultChoices);
  useCurrentChoices(step, setChoices);
  const { mode } = useMode();
  const appWindow = useAppWindow();
  useWindowDrag(appWindow);
  // The admin names their account last, where there are accounts.
  const active = useActiveAccount();
  const admin = active?.role === "admin" ? active : null;
  // The household's accounts came from the other device: nothing to name.
  // Its games can be copied once this device's folder is chosen (#276).
  const questions: Step[] = fromDevice
    ? ["language", "folder", "games", "sources", "mode"]
    : admin
      ? [...basics, "account"]
      : basics;
  const order: Step[] = [
    "welcome",
    ...(fromDevice ? (["device"] as Step[]) : []),
    ...questions,
    "setup",
  ];

  const go = (next: Step) => {
    sounds.play("step");
    setStep(next);
  };
  const next = () => go(order[order.indexOf(step) + 1] ?? "setup");
  const back = () => {
    const index = order.indexOf(step);
    if (questions.includes(step) && index > 0) go(order[index - 1] as Step);
  };
  const skip = () => {
    void markCompleted(settingsApi).finally(onDone);
  };
  const toggleSounds = () => {
    const on = !soundsOn;
    setMuted(!on);
    if (!on) sounds.silence();
    if (prefs.settings) prefs.setSettings((current) => ({ ...current, sounds: on }));
  };

  // The other device's admin has a PIN: it's asked for here before setup
  // goes on, as at any start.
  if (
    fromDevice &&
    accountList &&
    accountList.accounts.length > 0 &&
    !accountList.accounts.some((account) => account.id === accountList.activeId)
  )
    return <AccountPicker list={accountList} />;

  return (
    <InputProvider console={mode === "console"} locationKey={step} onBack={back}>
      <PathPickerProvider>
        <div
          data-cs="onboarding"
          data-step={step}
          className="relative flex h-full flex-col overflow-clip"
        >
          <div data-cs="backdrop" aria-hidden />
          <header
            data-part="titlebar"
            data-window-drag
            className="relative z-10 flex h-topbar shrink-0 items-center justify-end gap-2 pr-4 pl-6"
          >
            {questions.includes(step) && (
              <ol
                data-part="progress"
                aria-label={t({
                  message: `Step ${{ step: questions.indexOf(step) + 1 }} of ${{ total: questions.length }}`,
                  comment: "First-run setup's progress dots",
                })}
                className="pointer-events-none absolute left-1/2 flex -translate-x-1/2 gap-2"
              >
                {questions.map((question) => (
                  <li
                    key={question}
                    data-status={
                      question === step
                        ? "active"
                        : order.indexOf(question) < order.indexOf(step)
                          ? "done"
                          : "next"
                    }
                    className="h-1 w-8 rounded-full bg-border-strong transition-colors duration-(--cs-motion-duration-base) data-[status=active]:bg-accent-alt data-[status=done]:bg-accent"
                  />
                ))}
              </ol>
            )}
            <AriaButton
              data-part="sound"
              aria-label={soundsOn ? t`Turn sounds off` : t`Turn sounds on`}
              onPress={toggleSounds}
              className="grid size-8 place-items-center rounded-control text-fg-muted data-focus-visible:focus-ring data-hovered:bg-surface-hover data-hovered:text-fg"
            >
              {soundsOn ? <Volume2 className="size-4" /> : <VolumeX className="size-4" />}
            </AriaButton>
            {step !== "setup" && (
              <Button data-part="skip" variant="ghost" size="sm" onPress={skip}>
                <Trans>Skip setup</Trans>
              </Button>
            )}
            {appWindow?.frame === "custom" && mode === "desktop" && (
              <WindowControls window={appWindow} />
            )}
          </header>
          <main
            data-part="main"
            data-focus-section="main"
            className="relative z-10 flex min-h-0 flex-1 flex-col items-center overflow-y-auto px-6 pb-6"
          >
            <div
              key={step}
              // The welcome sits in the middle; the other steps keep their
              // heading in one place, however tall their content grows.
              className={`${step === "welcome" ? "my-auto" : "mt-[8vh]"} w-full max-w-2xl py-6`}
            >
              {step === "welcome" && (
                <Welcome
                  onShown={() => sounds.play("start")}
                  onStart={() => {
                    sounds.unlock();
                    setStep("language");
                  }}
                  onFromDevice={
                    devices
                      ? () => {
                          sounds.unlock();
                          setFromDevice(true);
                          setStep("device");
                        }
                      : undefined
                  }
                />
              )}
              {step === "device" && (
                <>
                  <DeviceStep onNext={next} />
                  <PairingHost />
                </>
              )}
              {step === "language" && <LanguageStep onNext={next} />}
              {step === "folder" && <FolderStep onNext={next} />}
              {step === "games" && <GamesStep onNext={next} />}
              {step === "sources" && (
                <SourcesStep choices={choices} onChange={setChoices} onNext={next} />
              )}
              {step === "mode" && <ModeStep onNext={next} last={!admin} />}
              {step === "account" && admin && <AccountStep account={admin} onNext={next} />}
              {step === "setup" && <SetupStep choices={choices} sounds={sounds} onDone={onDone} />}
            </div>
          </main>
        </div>
      </PathPickerProvider>
    </InputProvider>
  );
}

/** Setup run again starts from what is set now, not the defaults. Only
 * before the questions about it, so it never undoes the user's answers. */
function useCurrentChoices(step: Step, set: (update: (choices: Choices) => Choices) => void) {
  const discovery = useSettingsSection<{ enabled: boolean }>("core.discovery").settings;
  const history = useSettingsSection<{ seedPlaytime: boolean }>("discovery.import").settings;
  const metadata = useSettingsSection<{ online: boolean }>("core.metadata").settings;
  const early = step === "welcome" || step === "language" || step === "folder";
  useEffect(() => {
    if (!early) return;
    set((choices) => ({
      ...choices,
      pcGames: discovery?.enabled ?? choices.pcGames,
      playTime: history?.seedPlaytime ?? choices.playTime,
      online: metadata?.online ?? choices.online,
    }));
  }, [early, discovery?.enabled, history?.seedPlaytime, metadata?.online, set]);
}

function Welcome({
  onShown,
  onStart,
  onFromDevice,
}: {
  onShown(): void;
  onStart(): void;
  /** Set up from another device, where devices can pair. */
  onFromDevice?: () => void;
}) {
  const start = useRef<HTMLButtonElement>(null);
  useDefaultFocus(start);
  // The startup sound comes in with the logo.
  const shown = useRef(onShown);
  useEffect(() => shown.current(), []);
  return (
    <section data-part="welcome" className="flex flex-col items-center text-center">
      <div data-part="logo" className="relative mb-10 animate-rise">
        <div
          aria-hidden
          className="absolute inset-0 -z-10 scale-150 rounded-full bg-glow opacity-60 blur-3xl"
        />
        <img
          src="/caskade.svg"
          alt=""
          className="size-28 drop-shadow-[0_0_24px_var(--cs-color-glow)]"
        />
      </div>
      <h1 className="animate-rise font-display text-5xl font-semibold tracking-[0.4em] [--i:3]">
        CASKADE
      </h1>
      <AriaButton
        ref={start}
        data-part="start"
        data-focus-default
        onPress={onStart}
        className="mt-12 h-12 animate-rise rounded-control bg-linear-to-r from-accent to-accent-strong px-10 font-display text-base font-semibold tracking-widest text-accent-fg uppercase transition-[filter,box-shadow,scale] duration-(--cs-motion-duration-fast) [--i:8] data-focus-visible:focus-ring data-hovered:shadow-[0_0_28px_-4px] data-hovered:shadow-glow data-hovered:brightness-115 data-pressed:scale-[0.97]"
      >
        <Trans comment="First-run setup's welcome button">Get started</Trans>
      </AriaButton>
      {onFromDevice && (
        <AriaButton
          data-part="from-device"
          onPress={onFromDevice}
          className="mt-4 animate-rise rounded-control px-6 py-2 text-sm text-fg-muted [--i:9] data-focus-visible:focus-ring data-hovered:bg-surface-hover data-hovered:text-fg"
        >
          <Trans>Set up from another device</Trans>
        </AriaButton>
      )}
    </section>
  );
}

/** The heading every question shares. */
export function StepHeading({ title, description }: { title: string; description: string }) {
  return (
    <div className="mb-8 text-center">
      <h1 className="animate-rise font-display text-3xl font-semibold">{title}</h1>
      <p className="mt-2 animate-rise text-fg-muted [--i:2]">{description}</p>
    </div>
  );
}
