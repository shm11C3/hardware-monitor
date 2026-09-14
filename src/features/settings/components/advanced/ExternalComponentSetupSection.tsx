import { platform } from "@tauri-apps/plugin-os";
import { DownloadIcon } from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { NeedRestart } from "@/components/shared/System";
import { Button } from "@/components/ui/button";
import { Label } from "@/components/ui/label";
import { Skeleton } from "@/components/ui/skeleton";
import { Spinner } from "@/components/ui/spinner";
import { useTauriDialog } from "@/hooks/useTauriDialog";
import {
  commands,
  type ExternalComponent,
  type ExternalComponentSetupFailureStage,
  type ExternalComponentSetupResult,
  type ExternalComponentSetupStatus,
} from "@/rspc/bindings";
import { isError } from "@/types/result";

type ComponentEntry = {
  component: ExternalComponent;
  status: ExternalComponentSetupStatus | null;
};

/**
 * External Component Setup (ADR 0024): per-component state and the explicit
 * install action. Windows only; other platforms keep the documentation link.
 */
export const ExternalComponentSetupSection = () => {
  const { t } = useTranslation();
  const { error } = useTauriDialog();
  const [entries, setEntries] = useState<ComponentEntry[] | null>(null);
  const [runningComponent, setRunningComponent] =
    useState<ExternalComponent | null>(null);
  const [lastResult, setLastResult] =
    useState<ExternalComponentSetupResult | null>(null);
  const [restartDialogOpen, setRestartDialogOpen] = useState(false);

  const isWindows = platform() === "windows";

  const loadEntries = useCallback(async () => {
    const components = await commands.getExternalComponentSetupComponents();
    const loaded = await Promise.all(
      components.map(async (component): Promise<ComponentEntry> => {
        const result =
          await commands.getExternalComponentSetupStatus(component);
        if (isError(result)) {
          console.error(
            "Failed to read external component setup status:",
            result.error,
          );
          return { component, status: null };
        }
        return { component, status: result.data };
      }),
    );
    setEntries(loaded);
  }, []);

  useEffect(() => {
    if (!isWindows) return;
    void loadEntries();
  }, [isWindows, loadEntries]);

  if (!isWindows) {
    return null;
  }

  const handleSetup = async (component: ExternalComponent) => {
    setRunningComponent(component);
    setLastResult(null);
    try {
      const result = await commands.runExternalComponentSetup(component);
      if (isError(result)) {
        await error(
          t("pages.settings.advanced.externalComponentSetup.result.failed", {
            detail: result.error,
          }),
        );
        return;
      }

      setLastResult(result.data);
      setEntries((current) =>
        (current ?? []).map((entry) =>
          entry.component === component
            ? { component, status: result.data.status }
            : entry,
        ),
      );

      if (result.data.outcome === "installed") {
        setRestartDialogOpen(true);
      } else if (result.data.outcome === "failed") {
        await error(failureMessage(t, result.data));
      }
    } finally {
      setRunningComponent(null);
    }
  };

  return (
    <div className="flex w-full flex-col gap-4 py-6 xl:w-1/2">
      <div className="space-y-0.5">
        <Label className="text-lg">
          {t("pages.settings.advanced.externalComponentSetup.title")}
        </Label>
        <p className="text-muted-foreground text-sm">
          {t("pages.settings.advanced.externalComponentSetup.description")}
        </p>
      </div>

      {entries === null ? (
        <Skeleton className="h-24 w-full rounded-md" />
      ) : (
        entries.map((entry) => (
          <ComponentCard
            key={entry.component}
            entry={entry}
            running={runningComponent === entry.component}
            disabled={runningComponent !== null}
            result={
              lastResult?.component === entry.component ? lastResult : null
            }
            onSetup={() => void handleSetup(entry.component)}
          />
        ))
      )}

      <NeedRestart
        alertOpen={restartDialogOpen}
        setAlertOpen={setRestartDialogOpen}
        description={t(
          "pages.settings.advanced.externalComponentSetup.restartDescription",
        )}
      />
    </div>
  );
};

type Translate = ReturnType<typeof useTranslation>["t"];

const FAILURE_STAGE_KEYS = {
  stateUnknown:
    "pages.settings.advanced.externalComponentSetup.failureStage.stateUnknown",
  stagingDirectory:
    "pages.settings.advanced.externalComponentSetup.failureStage.stagingDirectory",
  downloadRuntime:
    "pages.settings.advanced.externalComponentSetup.failureStage.downloadRuntime",
  verifyRuntime:
    "pages.settings.advanced.externalComponentSetup.failureStage.verifyRuntime",
  startInstaller:
    "pages.settings.advanced.externalComponentSetup.failureStage.startInstaller",
  installerExit:
    "pages.settings.advanced.externalComponentSetup.failureStage.installerExit",
  downloadModules:
    "pages.settings.advanced.externalComponentSetup.failureStage.downloadModules",
  verifyModules:
    "pages.settings.advanced.externalComponentSetup.failureStage.verifyModules",
  archiveContents:
    "pages.settings.advanced.externalComponentSetup.failureStage.archiveContents",
  placeModules:
    "pages.settings.advanced.externalComponentSetup.failureStage.placeModules",
  incomplete:
    "pages.settings.advanced.externalComponentSetup.failureStage.incomplete",
  unsupportedPlatform:
    "pages.settings.advanced.externalComponentSetup.failureStage.unsupportedPlatform",
  panicked:
    "pages.settings.advanced.externalComponentSetup.failureStage.panicked",
  other: "pages.settings.advanced.externalComponentSetup.failureStage.other",
} as const satisfies Record<ExternalComponentSetupFailureStage, string>;

/**
 * The elevated setup process reports only an exit code, so the stage is the
 * user-facing explanation; free-text detail exists only for failures the app
 * process itself detected.
 */
const failureMessage = (
  t: Translate,
  result: ExternalComponentSetupResult,
): string => {
  const stage = t(FAILURE_STAGE_KEYS[result.failureStage ?? "other"]);
  return result.detail
    ? t("pages.settings.advanced.externalComponentSetup.result.failed", {
        detail: `${stage} (${result.detail})`,
      })
    : t("pages.settings.advanced.externalComponentSetup.result.failed", {
        detail: stage,
      });
};

/** Copy keys per component; a component without copy has no setup plan. */
const componentCopyKeys = (component: ExternalComponent) => {
  switch (component) {
    case "pawnio":
      return {
        name: "pages.settings.advanced.externalComponentSetup.components.pawnio.name",
        description:
          "pages.settings.advanced.externalComponentSetup.components.pawnio.description",
      } as const;
    case "smartctl":
      return null;
  }
};

type ComponentCardProps = {
  entry: ComponentEntry;
  running: boolean;
  disabled: boolean;
  result: ExternalComponentSetupResult | null;
  onSetup: () => void;
};

const ComponentCard = ({
  entry,
  running,
  disabled,
  result,
  onSetup,
}: ComponentCardProps) => {
  const { t } = useTranslation();
  const { component, status } = entry;
  const copy = componentCopyKeys(component);
  const componentName = copy ? t(copy.name) : component;

  const presentCount = status?.moduleFiles.filter((f) => f.present).length ?? 0;
  const missingFiles =
    status?.moduleFiles.filter((f) => !f.present).map((f) => f.fileName) ?? [];

  const actionLabel =
    status?.runtime.state === "installed"
      ? t("pages.settings.advanced.externalComponentSetup.installMissing")
      : t("pages.settings.advanced.externalComponentSetup.install");

  const runtimeLine = (setupStatus: ExternalComponentSetupStatus): string => {
    switch (setupStatus.runtime.state) {
      case "installed":
        return setupStatus.runtime.version
          ? t(
              "pages.settings.advanced.externalComponentSetup.runtimeInstalled",
              { version: setupStatus.runtime.version },
            )
          : t(
              "pages.settings.advanced.externalComponentSetup.runtimeInstalledUnknownVersion",
            );
      case "notInstalled":
        return t(
          "pages.settings.advanced.externalComponentSetup.runtimeMissing",
          { version: setupStatus.pinnedRuntimeVersion },
        );
      case "unknown":
        return t(
          "pages.settings.advanced.externalComponentSetup.runtimeUnknown",
        );
    }
  };

  const resultMessage = (setupResult: ExternalComponentSetupResult): string => {
    switch (setupResult.outcome) {
      case "installed":
        return t(
          "pages.settings.advanced.externalComponentSetup.result.installed",
          { component: componentName },
        );
      case "rebootRequired":
        return t(
          "pages.settings.advanced.externalComponentSetup.result.rebootRequired",
          { component: componentName },
        );
      case "alreadyInstalled":
        return t(
          "pages.settings.advanced.externalComponentSetup.result.alreadyInstalled",
          { component: componentName },
        );
      case "cancelled":
        return t(
          "pages.settings.advanced.externalComponentSetup.result.cancelled",
        );
      case "failed":
        return failureMessage(t, setupResult);
    }
  };

  const blocked = status !== null && status.setupBlocker !== null;

  return (
    <div className="rounded-md border border-border p-4">
      <div className="flex flex-col gap-3 sm:flex-row sm:items-start sm:justify-between">
        <div className="space-y-1">
          <p className="font-semibold">{componentName}</p>
          {copy && (
            <p className="text-muted-foreground text-sm">
              {t(copy.description)}
            </p>
          )}
          {status === null ? (
            <p className="text-destructive text-sm">
              {t("pages.settings.advanced.externalComponentSetup.statusError")}
            </p>
          ) : (
            <ul className="text-sm">
              <li>{runtimeLine(status)}</li>
              <li>
                {t(
                  "pages.settings.advanced.externalComponentSetup.moduleFiles",
                  {
                    present: presentCount,
                    total: status.moduleFiles.length,
                  },
                )}
                {missingFiles.length > 0 && (
                  <span className="text-muted-foreground">
                    {" "}
                    {t(
                      "pages.settings.advanced.externalComponentSetup.missingModuleFiles",
                      { files: missingFiles.join(", ") },
                    )}
                  </span>
                )}
              </li>
              {blocked && (
                <li className="text-destructive">
                  {t(
                    "pages.settings.advanced.externalComponentSetup.setupBlocked",
                    { detail: status.setupBlocker },
                  )}
                </li>
              )}
            </ul>
          )}
        </div>

        {status !== null && (
          <Button
            type="button"
            variant={status.complete ? "secondary" : "default"}
            disabled={status.complete || blocked || disabled}
            onClick={onSetup}
          >
            {running ? (
              <Spinner className="size-4" />
            ) : (
              <DownloadIcon className="size-4" />
            )}
            {status.complete
              ? t("pages.settings.advanced.externalComponentSetup.complete")
              : actionLabel}
          </Button>
        )}
      </div>

      {running && (
        <p className="pt-3 text-muted-foreground text-sm" role="status">
          {t("pages.settings.advanced.externalComponentSetup.running")}
        </p>
      )}

      {!running && result !== null && (
        <p className="pt-3 text-sm" role="status">
          {resultMessage(result)}
        </p>
      )}
    </div>
  );
};
