import { ArrowSquareOutIcon } from "@phosphor-icons/react";
import { useTranslation } from "react-i18next";
import { externalComponentGuidanceDocsBaseUrl } from "@/components/shared/externalComponentGuidance";
import { Button } from "@/components/ui/button";
import { Label } from "@/components/ui/label";
import { Skeleton } from "@/components/ui/skeleton";
import { Switch } from "@/components/ui/switch";
import { useTauriDialog } from "@/hooks/useTauriDialog";
import { useTauriStore } from "@/hooks/useTauriStore";
import { openURL } from "@/lib/openUrl";
import { ElevatedStartupModeToggle } from "./ElevatedStartupModeToggle";
import { ExternalComponentSetupSection } from "./ExternalComponentSetupSection";

export const AdvancedSettings = () => {
  const { t, i18n } = useTranslation();
  const { error } = useTauriDialog();
  const [showGpuUsageSource, setShowGpuUsageSource, isPending] = useTauriStore(
    "showGpuUsageSource",
    false,
  );

  const handleOpenExternalComponentDocs = async () => {
    try {
      await openURL(
        externalComponentGuidanceDocsBaseUrl(
          i18n.resolvedLanguage ?? i18n.language,
        ),
      );
    } catch (err) {
      console.error("Failed to open external components documentation:", err);
      await error(t("pages.settings.advanced.openExternalComponentsDocsError"));
    }
  };

  return (
    <div className="p-4">
      <h3 className="py-3 font-bold text-2xl">
        {t("pages.settings.advanced.name")}
      </h3>
      <div className="px-4">
        <ElevatedStartupModeToggle />
        <div className="flex w-full items-center justify-between space-x-4 py-6 xl:w-1/3">
          <div className="space-y-0.5">
            <Label htmlFor="showGpuUsageSource" className="text-lg">
              {t("pages.settings.advanced.showGpuUsageSource")}
            </Label>
            <p className="text-muted-foreground text-sm">
              {t("pages.settings.advanced.showGpuUsageSourceDescription")}
            </p>
          </div>

          {!isPending ? (
            <Switch
              id="showGpuUsageSource"
              checked={showGpuUsageSource ?? false}
              onCheckedChange={setShowGpuUsageSource}
            />
          ) : (
            <Skeleton className="h-6 w-11 rounded-full" />
          )}
        </div>
        <ExternalComponentSetupSection />
        <div className="flex w-full flex-col items-start gap-4 py-6 sm:flex-row sm:items-center sm:justify-between xl:w-1/2">
          <div className="space-y-0.5">
            <Label className="text-lg">
              {t("pages.settings.advanced.externalComponents")}
            </Label>
            <p className="text-muted-foreground text-sm">
              {t("pages.settings.advanced.externalComponentsDescription")}
            </p>
          </div>

          <Button
            onClick={() => void handleOpenExternalComponentDocs()}
            type="button"
            variant="secondary"
          >
            {t("pages.settings.advanced.openExternalComponentsDocs")}
            <ArrowSquareOutIcon size={16} />
          </Button>
        </div>
      </div>
    </div>
  );
};
