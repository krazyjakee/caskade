/** First-run setup's questions: where games go, what to look for, and how
 * the user plays. */
import { Checkbox, SegmentedControl, Switch, TextField } from "@caskade/ui";
import { Trans, useLingui } from "@lingui/react/macro";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { isTauri } from "@tauri-apps/api/core";
import {
  FolderOpen,
  Gamepad2,
  HardDrive,
  Image,
  type LucideIcon,
  Monitor,
  MonitorPlay,
  Server,
  Tv,
} from "lucide-react";
import { type ReactNode, useRef, useState } from "react";
import { Button as AriaButton, Radio, RadioGroup } from "react-aria-components";
import { commands } from "../api/bindings";
import { formatPath } from "../api/presentation";
import { errorMessage } from "../api/reply";
import type { RommAuth, RommConnection } from "../api/romm";
import { useStorageApi } from "../api/StorageProvider";
import { formatBytes } from "../api/storage";
import { usePathPicker } from "../components/pathBrowser/PathPicker";
import { useDefaultFocus } from "../input";
import { type Mode, useMode } from "../shell/console/mode";
import { useEmulationApi } from "../views/settings/emulators/EmulationProvider";
import { useRommApi } from "../views/settings/romm/useRomm";
import { StepHeading } from "./Onboarding";
import { type Choices, emptyRommChoice, rommServer } from "./run";

const primary =
  "inline-flex h-11 items-center justify-center gap-2 rounded-control bg-linear-to-r from-accent to-accent-strong px-8 font-semibold text-accent-fg transition-[filter,box-shadow,scale,opacity] duration-(--cs-motion-duration-fast) data-disabled:opacity-40 data-focus-visible:focus-ring data-hovered:shadow-[0_0_22px_-4px] data-hovered:shadow-glow data-hovered:brightness-115 data-pressed:scale-[0.97]";
const secondary =
  "inline-flex h-9 items-center justify-center gap-2 rounded-control border border-border-strong bg-surface-overlay/70 px-4 text-sm font-medium text-fg data-disabled:opacity-40 data-focus-visible:focus-ring data-hovered:border-accent-alt/50 data-hovered:bg-surface-hover";

/** Continue at the bottom of a question; a pad starts on it. */
export function Continue({
  onPress,
  isDisabled = false,
  children,
}: {
  onPress(): void;
  isDisabled?: boolean;
  children?: ReactNode;
}) {
  const ref = useRef<HTMLButtonElement>(null);
  useDefaultFocus(ref, !isDisabled);
  return (
    <div className="mt-10 flex animate-rise justify-center [--i:8]">
      <AriaButton
        ref={ref}
        data-part="next"
        data-focus-default
        isDisabled={isDisabled}
        onPress={onPress}
        className={primary}
      >
        {children ?? <Trans>Continue</Trans>}
      </AriaButton>
    </div>
  );
}

export function FolderStep({ onNext }: { onNext(): void }) {
  const { t } = useLingui();
  const given = useStorageApi();
  const storage = () => {
    if (!given) throw new Error(t`Storage is unavailable.`);
    return given;
  };
  const client = useQueryClient();
  const picker = usePathPicker();
  const roots = useQuery({ queryKey: ["storage", "roots"], queryFn: () => storage().list() });
  const choose = useMutation({
    mutationFn: async () => {
      const api = storage();
      const path = await picker.pick(
        { kind: "folder", title: t`Where should new games go?`, startAt: current?.root.path },
        () => api.chooseDirectory(),
      );
      if (!path) return;
      const added = await api.add(path);
      await api.setDefault(added.root.id);
    },
    onSettled: () => client.invalidateQueries({ queryKey: ["storage"] }),
  });
  const current = roots.data?.find((root) => root.root.isDefault) ?? roots.data?.[0];
  const free = current?.capabilities.freeBytes ?? null;
  const total = current?.capabilities.totalBytes ?? null;
  const error = roots.error ?? choose.error;
  return (
    <section data-part="question" data-question="folder">
      <StepHeading
        title={t`Where should new games go?`}
        description={t`Games you install with Caskade are kept here. You can add more places later.`}
      />
      <div
        data-part="card"
        className="flex animate-rise items-center gap-4 rounded-panel border border-border-strong bg-surface-raised p-5 [--i:4]"
      >
        <HardDrive aria-hidden className="size-8 shrink-0 text-accent-alt" />
        <div className="min-w-0 flex-1">
          {current ? (
            <>
              <p className="truncate font-mono text-sm" title={formatPath(current.root.path)}>
                {formatPath(current.root.path)}
              </p>
              {free !== null && (
                <>
                  <p className="mt-1 text-sm text-fg-muted">
                    {total
                      ? t`${{ free: formatBytes(free) }} free of ${{ total: formatBytes(total) }}`
                      : t`${{ free: formatBytes(free) }} free`}
                  </p>
                  {total ? (
                    <div className="mt-2 h-1.5 overflow-hidden rounded-full bg-border-strong">
                      <div
                        className="h-full rounded-full bg-linear-to-r from-accent to-accent-alt"
                        style={{ width: `${Math.max(2, 100 - (free / total) * 100)}%` }}
                      />
                    </div>
                  ) : null}
                </>
              )}
            </>
          ) : (
            <p className="text-sm text-fg-muted">
              {roots.isPending ? t`Checking…` : t`Choose a folder for your games.`}
            </p>
          )}
        </div>
        <AriaButton
          data-part="choose"
          className={secondary}
          isDisabled={choose.isPending}
          onPress={() => choose.mutate()}
        >
          <FolderOpen className="size-4" />
          {current
            ? t({ message: "Change", context: "folder" })
            : t({ message: "Choose", context: "folder" })}
        </AriaButton>
      </div>
      {error && (
        <p role="alert" className="mt-3 text-center text-sm text-danger">
          {errorMessage(error)}
        </p>
      )}
      <Continue onPress={onNext} isDisabled={!current} />
    </section>
  );
}

/** One thing to look for: a switch, and what it needs once it's on. */
function Source({
  icon: Icon,
  title,
  description,
  isSelected,
  onChange,
  index,
  children,
}: {
  icon: LucideIcon;
  title: string;
  description: string;
  isSelected: boolean;
  onChange(value: boolean): void;
  index: number;
  children?: ReactNode;
}) {
  return (
    <div
      data-part="source"
      data-status={isSelected ? "on" : "off"}
      style={{ "--i": index + 3 } as React.CSSProperties}
      className="animate-rise rounded-panel border border-border bg-surface-raised p-4 transition-colors duration-(--cs-motion-duration-base) data-[status=on]:border-accent/50"
    >
      <div className="flex items-center gap-4">
        <Icon aria-hidden className="size-6 shrink-0 text-accent-alt" />
        <div className="min-w-0 flex-1">
          <p className="font-medium">{title}</p>
          <p className="text-sm text-fg-muted">{description}</p>
        </div>
        <Switch aria-label={title} isSelected={isSelected} onChange={onChange} />
      </div>
      {isSelected && children && <div className="mt-4 ml-10 flex flex-col gap-3">{children}</div>}
    </div>
  );
}

export function SourcesStep({
  choices,
  onChange,
  onNext,
}: {
  choices: Choices;
  onChange(choices: Choices): void;
  onNext(): void;
}) {
  const { t } = useLingui();
  const emulation = useEmulationApi();
  const picker = usePathPicker();
  const romm = useRommApi();
  const [retro, setRetro] = useState(choices.romFolder !== null);
  const [folder, setFolder] = useState(choices.romFolder);
  const [useRomm, setUseRomm] = useState(choices.romm !== null);
  const [server, setServer] = useState(choices.romm ?? emptyRommChoice);
  const [checked, setChecked] = useState<RommConnection | null>(null);
  const check = useMutation({
    mutationFn: () => romm.testServer(rommServer(server, []), server.secret.trim() || null),
    onSuccess: setChecked,
    onError: () => setChecked(null),
  });
  const set = (patch: Partial<Choices>) => onChange({ ...choices, ...patch });
  const pick = useMutation({
    mutationFn: () =>
      picker.pick({ kind: "folder", title: t`Your ROM folder`, startAt: folder }, () =>
        emulation.choosePath("folder"),
      ),
    onSuccess: (path) => {
      if (path) setFolder(path);
    },
  });
  // A token server can't sign in without its token.
  const rommReady =
    server.url.trim() !== "" && (server.auth === "password" || server.secret.trim() !== "");
  const ready = (!retro || folder !== null) && (!useRomm || rommReady);
  const next = () => {
    onChange({
      ...choices,
      romFolder: retro ? folder : null,
      romm: useRomm ? server : null,
    });
    onNext();
  };
  const field = (key: "url" | "username" | "secret") => (value: string) => {
    setChecked(null);
    setServer((old) => ({ ...old, [key]: value }));
  };
  return (
    <section data-part="question" data-question="sources">
      <StepHeading
        title={t`What should Caskade look for?`}
        description={t`Turn on what you have. Everything can be changed later in Settings.`}
      />
      <div className="flex flex-col gap-3">
        <Source
          index={0}
          icon={Monitor}
          title={t`Games on this PC`}
          description={t`From Steam, Epic, GOG, Heroic, Lutris and more.`}
          isSelected={choices.pcGames}
          onChange={(pcGames) => set({ pcGames })}
        >
          <Checkbox isSelected={choices.playTime} onChange={(playTime) => set({ playTime })}>
            <Trans>Keep my play time</Trans>
          </Checkbox>
        </Source>
        <Source
          index={1}
          icon={Gamepad2}
          title={t`Retro games`}
          description={t({
            message: "A ROMs folder with a folder per console, like snes or psx.",
            comment: "snes and psx are folder names; keep them as they are",
          })}
          isSelected={retro}
          onChange={setRetro}
        >
          <div className="flex items-center gap-3">
            <AriaButton className={secondary} onPress={() => pick.mutate()}>
              <FolderOpen className="size-4" />
              {folder ? t`Change folder` : t`Choose folder`}
            </AriaButton>
            {folder && (
              <span
                className="min-w-0 truncate font-mono text-sm text-fg-muted"
                title={formatPath(folder)}
              >
                {formatPath(folder)}
              </span>
            )}
          </div>
          {pick.error && (
            <p role="alert" className="text-sm text-danger">
              {errorMessage(pick.error)}
            </p>
          )}
        </Source>
        <Source
          index={2}
          icon={Server}
          title="RomM"
          description={t`Games from your own RomM server.`}
          isSelected={useRomm}
          onChange={setUseRomm}
        >
          <TextField
            label={t({ message: "Address", context: "server" })}
            placeholder={t({
              message: "https://romm.example.com",
              comment: "An example server address",
            })}
            value={server.url}
            onChange={field("url")}
          />
          <SegmentedControl
            aria-label={t`Sign in with`}
            options={
              [
                { id: "password", label: t`Username & password` },
                { id: "token", label: t`API token` },
              ] satisfies { id: RommAuth; label: string }[]
            }
            value={server.auth}
            onChange={(auth) => {
              setChecked(null);
              // A password isn't a token: the other one starts empty.
              setServer((old) => ({ ...old, auth, secret: "" }));
            }}
            className="self-start"
          />
          {server.auth === "password" ? (
            <div className="grid grid-cols-2 gap-3">
              <TextField
                label={t`Username`}
                autoComplete="username"
                value={server.username}
                onChange={field("username")}
              />
              <TextField
                label={t`Password`}
                type="password"
                value={server.secret}
                onChange={field("secret")}
              />
            </div>
          ) : (
            <TextField
              label={t`API token`}
              type="password"
              placeholder={t({
                message: "rmm_…",
                comment: "How a RomM API token starts; keep it as it is",
              })}
              value={server.secret}
              onChange={field("secret")}
            />
          )}
          <div className="flex items-center gap-3">
            <AriaButton
              className={secondary}
              isDisabled={!rommReady || check.isPending}
              onPress={() => check.mutate()}
            >
              {check.isPending
                ? t`Checking…`
                : t({ message: "Check", context: "action", comment: "Tests the RomM server" })}
            </AriaButton>
            {checked && (
              <span role="status" className="text-sm text-status-installed">
                {checked.version
                  ? t`Connected · RomM ${{ version: checked.version }}`
                  : t`Connected`}
              </span>
            )}
            {check.error && (
              <span role="alert" className="text-sm text-danger">
                {errorMessage(check.error)}
              </span>
            )}
          </div>
        </Source>
        <Source
          index={3}
          icon={Image}
          title={t`Artwork and game info`}
          description={t`Covers, descriptions and genres, looked up online.`}
          isSelected={choices.online}
          onChange={(online) => set({ online })}
        />
      </div>
      <Continue onPress={next} isDisabled={!ready} />
    </section>
  );
}

/** A pad is plugged in: a hint towards console mode. */
function usePads() {
  return useQuery({
    queryKey: ["input", "devices"],
    queryFn: () => commands.inputDevices(),
    enabled: isTauri(),
    staleTime: 10_000,
  });
}

function ModeCard({
  value,
  icon: Icon,
  title,
  description,
}: {
  value: Mode;
  icon: LucideIcon;
  title: string;
  description: string;
}) {
  return (
    <Radio
      value={value}
      data-part="mode"
      className="group flex animate-rise cursor-default flex-col items-center gap-3 rounded-panel border border-border-strong bg-surface-raised p-6 text-center transition-[border-color,box-shadow,background-color] duration-(--cs-motion-duration-base) data-focus-visible:focus-ring data-hovered:bg-surface-hover data-selected:border-accent data-selected:shadow-[0_0_28px_-8px] data-selected:shadow-glow"
    >
      <Icon
        aria-hidden
        className="size-12 text-fg-muted transition-colors duration-(--cs-motion-duration-base) group-data-selected:text-accent-alt"
      />
      <span className="font-display text-lg font-semibold">{title}</span>
      <span className="text-sm text-fg-muted">{description}</span>
    </Radio>
  );
}

/** `last`: setup starts from here; otherwise the account question follows. */
export function ModeStep({ onNext, last = true }: { onNext(): void; last?: boolean }) {
  const { t } = useLingui();
  const { mode, setMode } = useMode();
  const pads = usePads();
  const controller = pads.data?.[0]?.name;
  return (
    <section data-part="question" data-question="mode">
      <StepHeading
        title={t`How do you play?`}
        description={t({
          message: "Switch any time with the button at the top, or View on a controller.",
          comment: "View is a controller button",
        })}
      />
      <RadioGroup
        aria-label={t`How do you play?`}
        value={mode}
        onChange={(value) => setMode(value as Mode)}
        className="grid grid-cols-2 gap-4"
      >
        <ModeCard
          value="desktop"
          icon={MonitorPlay}
          title={t({ message: "Desktop", context: "mode" })}
          description={t`In a window, with mouse and keyboard.`}
        />
        <ModeCard
          value="console"
          icon={Tv}
          title={t`Console mode`}
          description={t`Full screen, made for a controller and a TV.`}
        />
      </RadioGroup>
      {(pads.data?.length ?? 0) > 0 && (
        <p className="mt-4 animate-rise text-center text-sm text-fg-muted [--i:6]">
          <Gamepad2 aria-hidden className="mr-1.5 inline size-4 text-accent-alt" />
          {controller ? t`${{ controller }} is connected.` : t`A controller is connected.`}
        </p>
      )}
      <Continue onPress={onNext}>
        {last ? (
          <Trans comment="Finishes first-run setup's questions and starts the work">Set up</Trans>
        ) : (
          <Trans>Continue</Trans>
        )}
      </Continue>
    </section>
  );
}
