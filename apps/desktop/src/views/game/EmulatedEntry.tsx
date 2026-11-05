import { Button, Select, StatusDot, TextField } from "@caskade/ui";
import { t } from "@lingui/core/macro";
import { Trans, useLingui } from "@lingui/react/macro";
import { Link } from "@tanstack/react-router";
import { useState } from "react";
import {
  type EmulatorTarget,
  type GameEmulation,
  useGameEmulation,
  useSetGameEmulation,
  useSplitArgs,
} from "../../api/gameEmulation";
import { formatPath } from "../../api/presentation";

/** Whether an emulated entry can start (asking at launch counts). */
export function canStart(game: GameEmulation | undefined) {
  return game?.readiness.type === "ready" || game?.readiness.type === "ask";
}

const targetKey = (target: EmulatorTarget) => {
  switch (target.type) {
    case "platform_default":
      return "platform_default";
    case "ask_emulator":
      return "ask_emulator";
    case "ask_profile":
      return `ask:${target.emulatorId}`;
    case "profile":
      return `profile:${target.emulatorId}:${target.profileId}`;
  }
};

const readinessTone = (game: GameEmulation) => {
  switch (game.readiness.type) {
    case "ready":
    case "ask":
      return "ok" as const;
    case "missing_bios":
    case "missing_files":
      return "warning" as const;
    default:
      return "danger" as const;
  }
};

const biosTone = (state: string | null, required: boolean) =>
  state === "found"
    ? ("ok" as const)
    : state === "wrong_hash"
      ? ("warning" as const)
      : state === "missing" && required
        ? ("danger" as const)
        : ("off" as const);

const biosLabel = (state: string | null) =>
  state === "found"
    ? t({ message: "Found", comment: "A BIOS file's state" })
    : state === "wrong_hash"
      ? t({ message: "Wrong dump", comment: "A BIOS file's state: the file's hash doesn't match" })
      : state === "missing"
        ? t({ message: "Missing", comment: "A file's state" })
        : t({ message: "Unknown", comment: "A BIOS file's state" });

/** Emulator settings, discs and BIOS status of one emulated entry (#122). */
export function EmulatedEntry({ entryId }: { entryId: string }) {
  const { t } = useLingui();
  const query = useGameEmulation(entryId);
  const set = useSetGameEmulation(entryId);
  const split = useSplitArgs();
  const game = query.data;
  const appended = game?.args.type === "append" ? game.args.args : [];
  const [argsText, setArgsText] = useState<string | null>(null);
  if (query.isPending)
    return (
      <p className="text-sm text-fg-muted">
        <Trans>Checking the emulator…</Trans>
      </p>
    );
  if (query.isError || !game) {
    return (
      <p role="alert" className="text-sm text-danger">
        {query.error?.message ?? t`Emulator settings are unavailable.`}
      </p>
    );
  }
  if (game.readiness.type === "not_emulated") return null;

  const resolved = game.target.type === "platform_default" ? game.profile : null;
  const profileLabel = (emulator: string, profile: string, available = true) =>
    available
      ? `${emulator} · ${profile}`
      : t({
          message: `${emulator} · ${profile} (missing files)`,
          comment: "An emulator and its profile, whose files are missing",
        });
  const resolvedProfile = resolved && profileLabel(resolved.emulatorName, resolved.profileName);
  const options = [
    {
      id: "platform_default",
      label: resolvedProfile
        ? t({
            message: `Platform default (${resolvedProfile})`,
            comment:
              "The platform's default emulator, then the emulator and profile it resolves to",
          })
        : t`Platform default`,
    },
    ...game.compatible.map((profile) => ({
      id: `profile:${profile.emulatorId}:${profile.profileId}`,
      label: profileLabel(profile.emulatorName, profile.profileName, profile.available),
    })),
    { id: "ask_emulator", label: t`Ask when launching` },
  ];
  const current = targetKey(game.target);
  if (!options.some((option) => option.id === current)) {
    options.push({ id: current, label: t`A removed emulator` });
  }
  const choose = (key: string) => {
    const [kind, emulatorId, profileId] = key.split(":");
    const target: EmulatorTarget =
      kind === "profile" && emulatorId && profileId
        ? { type: "profile", emulatorId, profileId }
        : kind === "ask" && emulatorId
          ? { type: "ask_profile", emulatorId }
          : kind === "ask_emulator"
            ? { type: "ask_emulator" }
            : { type: "platform_default" };
    set.mutate({ target, args: game.args });
  };
  const saveArgs = async () => {
    const text = argsText ?? appended.join(" ");
    const args = text.trim() ? await split.mutateAsync(text) : [];
    set.mutate(
      { target: game.target, args: args.length ? { type: "append", args } : { type: "default" } },
      { onSuccess: () => setArgsText(null) },
    );
  };
  const platform = game.platform?.short ?? game.platformId;
  const variable = "{RomPath}";
  // One disc image is the game's ROM; several are its discs.
  const discsLabel =
    game.discs.length > 1
      ? t({ message: "Discs", comment: "The heading over a game's disc images" })
      : t({ message: "ROM", comment: "The heading over a game's single ROM file" });

  return (
    <div
      data-cs="emulation"
      data-status={game.readiness.type}
      className="grid grid-cols-[repeat(auto-fit,minmax(min(100%,18rem),1fr))] gap-x-6 gap-y-3 text-sm"
    >
      <div className="col-span-full flex flex-wrap items-center gap-3">
        <span className="font-mono text-xs text-fg-muted">{platform}</span>
        <StatusDot tone={readinessTone(game)}>
          {game.readiness.type === "ready"
            ? t({ message: "Ready", comment: "An emulated game's state: it can start" })
            : game.readiness.type === "ask"
              ? t`Asks for an emulator when launching`
              : t`Needs setup`}
        </StatusDot>
      </div>
      {game.message && (
        <p
          role="status"
          data-part="readiness"
          className="col-span-full flex flex-wrap items-center gap-2 text-warning"
        >
          {game.message}
          <Link
            to="/settings"
            search={{ tab: "emulators", section: "platforms" }}
            className="font-semibold text-accent-alt underline-offset-2 data-hovered:underline"
          >
            <Trans>Configure</Trans>
          </Link>
        </p>
      )}
      <div className="grid content-start gap-3">
        <Select
          label={t`Launch with`}
          options={options}
          value={current}
          onChange={choose}
          isDisabled={set.isPending}
        />
        <div className="flex items-end gap-2">
          <TextField
            label={t`Extra arguments`}
            description={t({
              message: `Added after the profile's arguments; variables like ${variable} work.`,
              comment: "{variable} is a launch variable, e.g. {RomPath}",
            })}
            mono
            value={argsText ?? appended.join(" ")}
            onChange={setArgsText}
            className="flex-1"
          />
          <Button
            variant="secondary"
            isDisabled={argsText === null || set.isPending || split.isPending}
            onPress={() => void saveArgs()}
          >
            <Trans>Save</Trans>
          </Button>
        </div>
        {(set.isError || split.isError) && (
          <p role="alert" className="text-danger">
            {(set.error ?? split.error)?.message}
          </p>
        )}
      </div>
      {(game.discs.length > 0 || game.bios.length > 0) && (
        <div className="grid content-start gap-3">
          {game.discs.length > 0 && (
            <div>
              <h4 className="font-semibold">{discsLabel}</h4>
              <ul className="mt-1 grid gap-1" aria-label={discsLabel}>
                {game.discs.map((disc) => (
                  <li key={disc.path} data-part="disc" className="flex items-center gap-2">
                    <StatusDot tone={disc.exists ? "ok" : "danger"}>
                      {disc.name ??
                        (disc.exists
                          ? t({ message: "Present", comment: "A file's state" })
                          : t({ message: "Missing", comment: "A file's state" }))}
                    </StatusDot>
                    <span className="break-all font-mono text-xs text-fg-subtle">
                      {formatPath(disc.path)}
                    </span>
                  </li>
                ))}
              </ul>
            </div>
          )}
          {game.bios.length > 0 && (
            <div>
              <h4 className="font-semibold">{t({ message: "BIOS", context: "table column" })}</h4>
              <ul
                className="mt-1 grid gap-1"
                aria-label={t({ message: "BIOS", context: "table column" })}
              >
                {game.bios.map((bios) => (
                  <li key={bios.file} data-part="bios" className="flex items-center gap-2">
                    <StatusDot tone={biosTone(bios.state, bios.required)}>
                      {biosLabel(bios.state)}
                    </StatusDot>
                    <span className="font-mono text-xs">{bios.file}</span>
                    {!bios.required && (
                      <span className="text-xs text-fg-subtle">
                        <Trans comment="A BIOS file the platform can do without">optional</Trans>
                      </span>
                    )}
                  </li>
                ))}
              </ul>
            </div>
          )}
        </div>
      )}
    </div>
  );
}
