import {
  getSettings,
  setDisplayTargets,
  setGraphSize,
  setLanguage,
  setTheme,
} from "@/services/settingService";
import type { ChartDataType } from "@/types/hardwareDataType";
import type { Settings } from "@/types/settingsType";
import { atom, useAtom } from "jotai";
import { useEffect } from "react";
const settingsAtom = atom<Settings>({
  language: "en",
  theme: "light",
  display_targets: [],
  graphSize: "xl",
});

export const useSettingsAtom = () => {
  const mapSettingUpdater: {
    [K in keyof Settings]: (value: Settings[K]) => Promise<void>;
  } = {
    theme: setTheme,
    display_targets: setDisplayTargets,
    graphSize: setGraphSize,
    language: setLanguage,
  };

  const [settings, setSettings] = useAtom(settingsAtom);

  useEffect(() => {
    const loadSettings = async () => {
      const setting = await getSettings();
      setSettings(setting);
    };
    loadSettings();
  }, [setSettings]);

  const updateSettingAtom = async <K extends keyof Settings>(
    key: K,
    value: Settings[K],
  ) => {
    try {
      await mapSettingUpdater[key](value);
      setSettings((prev) => ({ ...prev, [key]: value }));
    } catch (e) {
      console.error(e);
    }
  };

  const toggleDisplayTarget = async (target: ChartDataType) => {
    const newTargets = settings.display_targets.includes(target)
      ? settings.display_targets.filter((t) => t !== target)
      : [...settings.display_targets, target];

    try {
      // [TODO] Result型を作りたい
      await setDisplayTargets(newTargets);
      setSettings((prev) => ({ ...prev, display_targets: newTargets }));
    } catch (e) {
      console.error(e);
    }
  };

  return {
    settings,
    toggleDisplayTarget,
    updateSettingAtom,
  };
};
