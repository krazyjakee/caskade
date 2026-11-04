import { Button, SegmentedControl } from "@caskade/ui";
import { msg, plural } from "@lingui/core/macro";
import { Trans, useLingui } from "@lingui/react/macro";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import {
  CloudDownload,
  Cpu,
  FolderSearch,
  Gamepad2,
  Layers,
  Plus,
  RefreshCw,
  ScanSearch,
  Server,
  ShieldCheck,
} from "lucide-react";
import { type ReactNode, useState } from "react";
import type { ScanSummary } from "../../../api/emulation";
import { formatNumber } from "../../../api/presentation";
import { ConfiguredEmulators } from "./ConfiguredEmulators";
import { ConfiguredPlatforms } from "./ConfiguredPlatforms";
import { DetectionReview, useDetection } from "./DetectionReview";
import { emulationKey, useEmulation, useEmulationApi, useScanners } from "./EmulationProvider";
import { BrowseEmulators, MissingEmulators } from "./EmulatorSetup";
import { EmulatorsFromDevices } from "./FromDevices";
import { ScannersSection } from "./ScannersSection";

/** Labels are descriptors, translated where they're shown. */
export const sections = [
  {
    id: "emulators",
    label: msg({ message: "Emulators", context: "emulation section" }),
    icon: Cpu,
  },
  { id: "browse", label: msg`Get emulators`, icon: CloudDownload },
  {
    id: "platforms",
    label: msg({ message: "Platforms", context: "emulation section" }),
    icon: Layers,
  },
  { id: "scanners", label: msg`ROM scanners`, icon: FolderSearch },
  { id: "romm", label: msg`RomM server`, icon: Server },
] as const;
export type EmulationSection = (typeof sections)[number]["id"];
/** "Scan all" outcome in one line. */
function Scanned({ summaries }: { summaries: ScanSummary[] }) {
  const { t } = useLingui();
  const failed = summaries.filter((summary) => summary.error);
  const added = summaries.reduce((total, summary) => total + summary.added, 0);
  const missing = summaries.reduce((total, summary) => total + summary.missing, 0);
  const scanners = plural(
    { scanners: summaries.length - failed.length },
    { one: "Scanned # scanner", other: "Scanned # scanners" },
  );
  const games = plural({ added }, { one: "# new game", other: "# new games" });
  return [
    t({
      message: `${{ scanners }}: ${{ games }}`,
      comment: "“Scanned 2 scanners: 5 new games”",
    }),
    missing ? t({ message: `${{ missing }} missing`, comment: "ROMs a scan no longer found" }) : "",
    failed.length
      ? t({
          message: `${{ failed: failed.length }} failed (${{ errors: failed.map((f) => f.error).join("; ") }})`,
          comment: "How many ROM scanners failed, and their errors",
        })
      : "",
  ]
    .filter(Boolean)
    .join(", ");
}
export function EmulationPanel({
  section,
  setSection,
  renderExtra,
  header,
}: {
  section: EmulationSection;
  setSection: (section: EmulationSection) => void;
  renderExtra?: (section: "romm") => ReactNode;
  /** Settings for all of emulation, in their own card under the overview (#54). */
  header?: ReactNode;
}) {
  const { t } = useLingui();
  const state = useEmulation();
  const api = useEmulationApi();
  const detection = useDetection();
  const client = useQueryClient();
  const scanners = useScanners();
  const scan = useMutation({
    mutationFn: () => api.scanAll(),
    onSuccess: () => {
      void client.invalidateQueries({ queryKey: emulationKey });
      void client.invalidateQueries({ queryKey: ["library"] });
    },
  });
  const automatic = scanners.data?.filter((row) => row.scanner.inLibraryUpdate).length ?? 0;
  const [selectedId, select] = useState<string | null>(null);
  const [adding, setAdding] = useState(false);
  const [detecting, setDetecting] = useState(false);
  const add = () => {
    setAdding(true);
    setSection("emulators");
  };
  const detect = () => {
    setDetecting(true);
    detection.action.mutate({ type: "start", folder: null });
  };
  if (state.isPending)
    return (
      <p data-settings="emulators" role="status" className="p-5 text-sm text-fg-muted">
        <Trans>Loading emulators…</Trans>
      </p>
    );
  if (state.error || !state.data)
    return (
      <div
        data-settings="emulators"
        role="alert"
        className="rounded-panel border border-border p-5 text-sm text-fg-muted"
      >
        <p>
          <Trans>Emulators could not be loaded.</Trans>
        </p>
        <p className="mt-2">{state.error?.message}</p>
        <Button
          className="mt-3"
          onPress={() => {
            void state.refetch();
          }}
        >
          <Trans>Try again</Trans>
        </Button>
      </div>
    );
  const snapshot = state.data;
  const stats = [
    {
      icon: Cpu,
      value: snapshot.stats.emulators,
      label: t({ message: "Emulators", context: "emulation section" }),
    },
    {
      icon: Layers,
      value: `${snapshot.stats.workingPlatforms} / ${snapshot.stats.platforms}`,
      label: t`Platforms ready`,
    },
    { icon: Gamepad2, value: formatNumber(snapshot.stats.roms), label: t`ROMs found` },
    {
      icon: ShieldCheck,
      value: snapshot.stats.biosIssues ?? t({ message: "Not checked", comment: "BIOS issues" }),
      label: t`BIOS issues`,
    },
  ];
  return (
    <div data-settings="emulators" data-cs="emulation-settings" className="space-y-4">
      <section
        data-cs="panel"
        aria-label={t`Emulation overview`}
        className="space-y-5 rounded-panel border border-border bg-surface-raised p-6"
      >
        <div className="flex flex-wrap items-center gap-5">
          <div className="chamfer grid size-16 place-items-center bg-surface-overlay">
            <Gamepad2 className="size-8 text-accent-alt" />
          </div>
          <div className="min-w-64 flex-1">
            <h2 className="font-display text-xl font-semibold">
              <Trans>Emulation</Trans>
            </h2>
            <p className="mt-1 text-sm text-fg-muted">
              <Trans>
                Configure emulators, choose platform defaults and find your ROM collection.
              </Trans>
            </p>
          </div>
          <div className="flex flex-wrap gap-2">
            <Button
              variant="primary"
              icon={ScanSearch}
              isDisabled={detection.action.isPending || detection.data?.status === "searching"}
              onPress={detect}
            >
              <Trans>Detect emulators</Trans>
            </Button>
            <Button icon={Plus} onPress={add}>
              <Trans>Add emulator</Trans>
            </Button>
            <Button
              icon={RefreshCw}
              isDisabled={automatic === 0 || scan.isPending}
              onPress={() => scan.mutate()}
            >
              {scan.isPending ? t`Scanning…` : t`Scan all`}
            </Button>
          </div>
        </div>
        <div className="grid gap-3 sm:grid-cols-2 xl:grid-cols-4">
          {stats.map(({ icon: Icon, value, label }) => (
            <div
              key={label}
              data-part="stat"
              className="chamfer flex items-center gap-3 bg-surface-overlay px-4 py-3"
            >
              <Icon className="size-6 text-accent-alt" />
              <div>
                <p className="font-mono text-lg font-semibold">{value}</p>
                <p className="text-xs text-fg-muted">{label}</p>
              </div>
            </div>
          ))}
        </div>
        {automatic === 0 && (
          <p className="text-xs text-fg-muted">
            <Trans>
              Scan all runs the ROM scanners included in library updates. Add one under ROM
              Scanners.
            </Trans>
          </p>
        )}
        {scan.error && (
          <p role="alert" className="text-sm text-danger">
            {scan.error.message}
          </p>
        )}
        {scan.data && (
          <p role="status" className="text-sm text-fg-muted">
            <Scanned summaries={scan.data} />
          </p>
        )}
        {detection.action.error && !detecting && (
          <p role="alert" className="text-sm text-danger">
            {detection.action.error.message}
          </p>
        )}
      </section>
      {header}
      {detecting && <DetectionReview close={() => setDetecting(false)} />}
      <SegmentedControl
        aria-label={t`Emulation section`}
        options={sections.map((row) => ({ ...row, label: t(row.label) }))}
        value={section}
        onChange={setSection}
        className="max-w-full flex-wrap self-start"
      />
      {section === "emulators" && <EmulatorsFromDevices />}
      {section === "emulators" && <MissingEmulators />}
      {section === "emulators" && (
        <div className="@container">
          <ConfiguredEmulators
            snapshot={snapshot}
            selectedId={selectedId}
            adding={adding}
            select={(id) => {
              select(id);
              setAdding(false);
            }}
            add={add}
            added={(id) => {
              select(id);
              setAdding(false);
            }}
            cancelAdd={() => setAdding(false)}
          />
        </div>
      )}
      {section === "browse" && <BrowseEmulators snapshot={snapshot} />}
      {section === "platforms" && <ConfiguredPlatforms snapshot={snapshot} detect={detect} />}
      {section === "scanners" && <ScannersSection snapshot={snapshot} />}
      {section === "romm" && renderExtra?.(section)}
    </div>
  );
}
