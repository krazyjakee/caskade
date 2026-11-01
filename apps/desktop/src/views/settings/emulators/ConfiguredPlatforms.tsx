import { Button, Select, SettingsSection, StatusDot } from "@caskade/ui";
import { Trans, useLingui } from "@lingui/react/macro";
import { Layers, ScanSearch } from "lucide-react";
import { useState } from "react";
import type { EmulationSnapshot, ProfileRef } from "../../../api/emulation";
import { BiosSummary, PathField } from "./EmulationFields";
import { useEmulationWrite } from "./EmulationProvider";
import { FetchBios } from "./EmulatorSetup";

export function ConfiguredPlatforms({
  snapshot,
  detect,
}: {
  snapshot: EmulationSnapshot;
  detect: () => void;
}) {
  const { t } = useLingui();
  const save = useEmulationWrite();
  const [folders, setFolders] = useState<Record<string, string>>({});
  const compatible = (id: string) =>
    snapshot.emulators.flatMap((emulator) =>
      emulator.profiles
        .filter((profile) => profile.resolved.platforms.includes(id))
        .map((profile) => ({ emulator, profile })),
    );
  const platforms = snapshot.platforms.filter(
    (platform) => platform.romCount > 0 || platform.default || compatible(platform.id).length > 0,
  );
  return (
    <SettingsSection
      icon={Layers}
      title={t({ message: "Platforms", context: "emulation section" })}
      description={t`Defaults apply to compatible games that use Platform default. BIOS folders are saved per platform.`}
    >
      {save.error && (
        <p role="alert" className="mt-3 text-sm text-danger">
          {save.error.message}
        </p>
      )}
      {!platforms.length ? (
        <div className="mt-4 space-y-3 rounded-control border border-dashed border-border p-6 text-sm text-fg-muted">
          <p>
            <Trans>No configured profiles or ROMs yet.</Trans>
          </p>
          <Button icon={ScanSearch} onPress={detect}>
            <Trans>Detect emulators</Trans>
          </Button>
        </div>
      ) : (
        <div className="mt-4 overflow-x-auto rounded-control border border-border">
          <table className="w-full text-left text-sm">
            <thead className="bg-surface-overlay text-xs text-fg-subtle">
              <tr>
                <th className="px-4 py-3 font-medium">
                  <Trans context="table column">Platform</Trans>
                </th>
                <th className="px-4 py-3 text-right font-medium">
                  <Trans context="table column">ROMs</Trans>
                </th>
                <th className="min-w-64 px-4 py-3 font-medium">
                  <Trans context="table column">Default emulator</Trans>
                </th>
                <th className="min-w-80 px-4 py-3 font-medium">
                  <Trans context="table column">BIOS</Trans>
                </th>
                <th className="px-4 py-3 font-medium">
                  <Trans context="table column">Extensions</Trans>
                </th>
              </tr>
            </thead>
            <tbody>
              {platforms.map((platform) => {
                const choices = compatible(platform.id);
                const key = (selected: ProfileRef) =>
                  `${selected.emulatorId}/${selected.profileId}`;
                const selected = choices.find(
                  (row) =>
                    row.emulator.id === platform.default?.emulatorId &&
                    row.profile.config.id === platform.default.profileId,
                );
                const folder = folders[platform.id] ?? platform.biosDir ?? "";
                return (
                  <tr
                    data-part="platform-row"
                    key={platform.id}
                    className="border-t border-border align-top"
                  >
                    <td className="px-4 py-4">
                      <p className="font-medium">{platform.name}</p>
                      <p className="mt-1 font-mono text-xs text-fg-subtle">{platform.short}</p>
                    </td>
                    <td className="px-4 py-4 text-right font-mono">{platform.romCount}</td>
                    <td className="space-y-2 px-4 py-4">
                      <Select
                        aria-label={t`Default emulator for ${{ platform: platform.name }}`}
                        value={platform.default ? key(platform.default) : "none"}
                        options={[
                          {
                            id: "none",
                            label: t({ message: "None", context: "default emulator" }),
                          },
                          ...choices.map((row) => ({
                            id: key({
                              emulatorId: row.emulator.id,
                              profileId: row.profile.config.id,
                            }),
                            label: `${row.emulator.name} · ${row.profile.resolved.name}`,
                          })),
                        ]}
                        isDisabled={save.isPending}
                        onChange={(value) => {
                          const choice = choices.find(
                            (row) =>
                              key({
                                emulatorId: row.emulator.id,
                                profileId: row.profile.config.id,
                              }) === value,
                          );
                          save.mutate((api) =>
                            api.setPlatform(
                              platform.id,
                              choice
                                ? {
                                    emulatorId: choice.emulator.id,
                                    profileId: choice.profile.config.id,
                                  }
                                : null,
                              platform.biosDir,
                            ),
                          );
                        }}
                      />
                      {selected && !selected.profile.resolved.available && (
                        <StatusDot tone="warning">
                          <Trans>Default profile is unavailable</Trans>
                        </StatusDot>
                      )}
                      {!selected && (
                        <p className="text-xs text-fg-subtle">
                          <Trans>Games ask which emulator to use.</Trans>
                        </p>
                      )}
                    </td>
                    <td className="space-y-3 px-4 py-4">
                      <BiosSummary files={platform.bios} />
                      {platform.default &&
                        platform.bios.some(
                          (file) => file.found === false || file.state === "wrong_hash",
                        ) && <FetchBios platformId={platform.id} />}
                      <PathField
                        label={t`BIOS folder for ${{ platform: platform.short }}`}
                        value={folder}
                        onChange={(value) =>
                          setFolders((old) => ({ ...old, [platform.id]: value }))
                        }
                      />
                      <Button
                        size="sm"
                        isDisabled={save.isPending || folder === (platform.biosDir ?? "")}
                        onPress={() =>
                          save.mutate((api) =>
                            api.setPlatform(platform.id, platform.default, folder || null),
                          )
                        }
                      >
                        <Trans>Save BIOS folder</Trans>
                      </Button>
                    </td>
                    <td className="max-w-64 px-4 py-4">
                      <span className="font-mono text-xs text-fg-muted">
                        {(selected?.profile.resolved.extensions ?? platform.extensions).join(
                          ", ",
                        ) || t({ message: "None", context: "file extensions" })}
                      </span>
                    </td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        </div>
      )}
    </SettingsSection>
  );
}
