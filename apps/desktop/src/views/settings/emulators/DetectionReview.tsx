import { Button, Checkbox, SettingsSection } from "@caskade/ui";
import { Trans, useLingui } from "@lingui/react/macro";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { ScanSearch, X } from "lucide-react";
import { useState } from "react";
import { type EmulationDetection, installKindLabel, installLocation } from "../../../api/emulation";
import { PathField } from "./EmulationFields";
import { detectionKey, useEmulationApi, useEmulationWrite } from "./EmulationProvider";

const newest = (old: EmulationDetection | undefined, next: EmulationDetection) =>
  old && old.id === next.id && old.revision > next.revision ? old : next;
export function useDetection() {
  const api = useEmulationApi();
  const client = useQueryClient();
  const state = useQuery({
    queryKey: detectionKey,
    queryFn: async () => newest(client.getQueryData(detectionKey), await api.detection()),
    refetchInterval: (query) => (query.state.data?.status === "searching" ? 1000 : false),
  });
  const action = useMutation({
    mutationFn: (request: { type: "start"; folder: string | null } | { type: "cancel" }) =>
      request.type === "start" ? api.detect(request.folder) : api.cancelDetection(),
    onSuccess: (next) =>
      client.setQueryData<EmulationDetection>(detectionKey, (old) => newest(old, next)),
  });
  return { ...state, action };
}
function Results({ snapshot, close }: { snapshot: EmulationDetection; close: () => void }) {
  const { t } = useLingui();
  const save = useEmulationWrite();
  const [picked, setPicked] = useState<Record<string, boolean>>({});
  const chosen = snapshot.candidates
    .filter(
      (row) =>
        !row.configuredAs && (picked[row.id] ?? row.profiles.some((profile) => profile.available)),
    )
    .map((row) => row.id);
  return (
    <div className="mt-4 space-y-3">
      {snapshot.candidates.length === 0 && (
        <p className="rounded-control border border-dashed border-border p-5 text-sm text-fg-subtle">
          {snapshot.status === "searching"
            ? t`Waiting for detection results…`
            : t`No matching emulators found. Try a portable emulator folder.`}
        </p>
      )}
      <ul className="space-y-2">
        {snapshot.candidates.map((row) => (
          <li
            key={row.id}
            data-part="detected"
            data-configured={Boolean(row.configuredAs)}
            className="rounded-control border border-border bg-surface-overlay/40 p-3"
          >
            <Checkbox
              isSelected={chosen.includes(row.id)}
              isDisabled={
                Boolean(row.configuredAs) || save.isPending || snapshot.status === "searching"
              }
              onChange={(selected) => setPicked((old) => ({ ...old, [row.id]: selected }))}
            >
              <span className="font-medium">{row.name}</span>
              {row.version && (
                <span className="font-mono text-xs text-fg-subtle"> · {row.version}</span>
              )}
              {row.configuredAs && (
                <span className="text-xs text-fg-subtle">
                  {" · "}
                  <Trans comment="A detected emulator Caskade already has">
                    Already configured
                  </Trans>
                </span>
              )}
            </Checkbox>
            <p className="mt-1 break-all font-mono text-xs text-fg-muted">
              {installKindLabel(row.install)} · {installLocation(row.install)}
            </p>
            <ul className="mt-2 flex flex-wrap gap-2 text-xs text-fg-subtle">
              {row.profiles.map((profile) => (
                <li key={profile.name}>
                  {profile.name}:{" "}
                  {profile.available
                    ? t({ message: "Available", comment: "A detected emulator's profile" })
                    : t`Missing required files`}
                </li>
              ))}
            </ul>
          </li>
        ))}
      </ul>
      {save.error && (
        <p role="alert" className="text-sm text-danger">
          {save.error.message}
        </p>
      )}
      <div className="flex justify-end gap-2">
        <Button isDisabled={save.isPending} onPress={close}>
          <Trans>Close</Trans>
        </Button>
        <Button
          variant="primary"
          isDisabled={snapshot.status !== "completed" || !chosen.length || save.isPending}
          onPress={() =>
            save.mutate((api) => api.importDetected(chosen, snapshot.revision), {
              onSuccess: close,
            })
          }
        >
          {save.isPending
            ? t`Importing…`
            : chosen.length
              ? t`Import selected (${{ count: chosen.length }})`
              : t`Import selected`}
        </Button>
      </div>
    </div>
  );
}
export function DetectionReview({ close }: { close: () => void }) {
  const { t } = useLingui();
  const state = useDetection();
  const [folder, setFolder] = useState("");
  const running = state.data?.status === "searching";
  return (
    <SettingsSection
      icon={ScanSearch}
      title={t`Detect emulators`}
      description={t`Reads standard locations and the optional folder. Detection does not launch emulators.`}
      aside={
        <Button
          variant="ghost"
          size="sm"
          aria-label={t`Close emulator detection`}
          icon={X}
          onPress={close}
        />
      }
    >
      <div className="mt-3 flex flex-wrap items-end gap-3">
        <div className="min-w-72 flex-1">
          <PathField label={t`Extra folder`} value={folder} onChange={setFolder} />
        </div>
        <Button
          isDisabled={running || state.action.isPending}
          onPress={() => state.action.mutate({ type: "start", folder: folder.trim() || null })}
        >
          <Trans>Search again</Trans>
        </Button>
        <Button
          isDisabled={!running || state.action.isPending}
          onPress={() => state.action.mutate({ type: "cancel" })}
        >
          <Trans>Cancel detection</Trans>
        </Button>
      </div>
      <p role="status" className="mt-3 text-sm text-fg-muted">
        {state.data?.message ?? t`Loading detection status…`}
        {running ? ` · ${t`Searching…`}` : ""}
      </p>
      {(state.error || state.action.error || state.data?.error) && (
        <p role="alert" className="mt-3 text-sm text-danger">
          {state.error?.message ?? state.action.error?.message ?? state.data?.error}
        </p>
      )}
      {state.data?.warnings.map((warning) => (
        <p key={warning} className="mt-2 text-xs text-warning">
          {warning}
        </p>
      ))}
      {state.data && <Results key={state.data.id ?? "idle"} snapshot={state.data} close={close} />}
    </SettingsSection>
  );
}
