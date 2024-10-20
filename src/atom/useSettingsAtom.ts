import {
  getSettings,
  setDisplayTargets,
  setGraphSize,
  setLanguage,
  setLineGraphBorder,
  setLineGraphColor,
  setLineGraphFill,
  setLineGraphMix,
  setLineGraphShowLegend,
  setLineGraphShowScale,
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
  lineGraphBorder: true,
  lineGraphFill: true,
  lineGraphColor: {
    cpu: "rgb(75, 192, 192)",
    memory: "rgb(255, 99, 132)",
    gpu: "rgb(255, 206, 86)",
  },
  lineGraphMix: false,
  lineGraphShowLegend: true,
  lineGraphShowScale: false,
  state: {
    display: "dashboard",
  },
});

export const useSettingsAtom = () => {
  const mapSettingUpdater: {
    [K in keyof Omit<Settings, "state" | "lineGraphColor">]: (
      value: Settings[K],
    ) => Promise<void>;
  } = {
    theme: setTheme,
    displayTargets: setDisplayTargets,
    graphSize: setGraphSize,
    language: setLanguage,
    lineGraphBorder: setLineGraphBorder,
    lineGraphFill: setLineGraphFill,
    lineGraphMix: setLineGraphMix,
    lineGraphShowLegend: setLineGraphShowLegend,
    lineGraphShowScale: setLineGraphShowScale,
  };

  const [settings, setSettings] = useAtom(settingsAtom);

  useEffect(() => {
    const loadSettings = async () => {
      const setting = await getSettings();
      setSettings(setting);
    };
    loadSettings();
  }, [setSettings]);

  const updateSettingAtom = async <
    K extends keyof Omit<Settings, "state" | "lineGraphColor">,
  >(
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

  /**
   * カラーコードを更新する
   *
   * @param key
   * @param value 16進数形式のカラーコード
   */
  const updateLineGraphColorAtom = async (
    key: keyof Settings["lineGraphColor"],
    value: string,
  ) => {
    try {
      const result = await setLineGraphColor(key, value);
      setSettings((prev) => ({
        ...prev,
        lineGraphColor: { ...prev.lineGraphColor, [key]: result },
      }));
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
    updateLineGraphColorAtom,
    updateStateAtom,
  };
};
