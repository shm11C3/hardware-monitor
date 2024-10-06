import {
  getSettings,
  setDisplayTargets,
  setGraphSize,
  setLanguage,
  setState,
  setTheme,
} from "@/services/settingService";
import type { ChartDataType } from "@/types/hardwareDataType";
import type { Settings } from "@/types/settingsType";
import { atom, useAtom } from "jotai";
import { useEffect } from "react";
const settingsAtom = atom<Settings>({
  language: "en",
  theme: "light",
  displayTargets: [],
  graphSize: "xl",
  state: {
    display: "dashboard",
  },
});

export const useSettingsAtom = () => {
  const mapSettingUpdater: {
    [K in keyof Omit<Settings, "state">]: (value: Settings[K]) => Promise<void>;
  } = {
    theme: setTheme,
    displayTargets: setDisplayTargets,
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

  const updateSettingAtom = async <K extends keyof Omit<Settings, "state">>(
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
    const newTargets = settings.displayTargets.includes(target)
      ? settings.displayTargets.filter((t) => t !== target)
      : [...settings.displayTargets, target];

    try {
      // [TODO] Result型を作りたい
      await setDisplayTargets(newTargets);
      setSettings((prev) => ({ ...prev, displayTargets: newTargets }));
    } catch (e) {
      console.error(e);
    }
  };

  const updateStateAtom = async <K extends keyof Settings["state"]>(
    key: K,
    value: Settings["state"][K],
  ) => {
    try {
      await setState(key, value);
    } catch (e) {
      console.error(e);
    }

    setSettings((prev) => ({
      ...prev,
      state: { ...prev.state, [key]: value },
    }));
  };

  return {
    settings,
    toggleDisplayTarget,
    updateSettingAtom,
    updateStateAtom,
  };
};
