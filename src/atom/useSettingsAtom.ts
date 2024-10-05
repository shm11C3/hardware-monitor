import {
  getSettings,
  setDisplayTargets,
  setGraphSize,
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
  const [settings, setSettings] = useAtom(settingsAtom);

  useEffect(() => {
    const loadSettings = async () => {
      const setting = await getSettings();
      setSettings(setting);
    };
    loadSettings();
  }, [setSettings]);

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

  const toggleTheme = async (theme: "light" | "dark") => {
    try {
      await setTheme(theme);
      setSettings((prev) => ({ ...prev, theme }));
    } catch (e) {
      console.error(e);
    }
  };

  const toggleGraphSize = async (size: Settings["graphSize"]) => {
    try {
      await setGraphSize(size);
      setSettings((prev) => ({ ...prev, graphSize: size }));
    } catch (e) {
      console.error(e);
    }
  };

  return { settings, toggleDisplayTarget, toggleTheme, toggleGraphSize };
};
