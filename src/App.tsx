import { useEffect, useState } from "react";
import ChartTemplate from "./template/Chart";
import "./index.css";
import {
  useErrorModalListener,
  useSettingsModalListener,
} from "@/hooks/useTauriEventListener";
import SettingsSheet from "@/template/SettingsSheet";
import { useHardwareInfoAtom } from "./atom/useHardwareInfoAtom";
import { useSettingsAtom } from "./atom/useSettingsAtom";
import { useDarkMode } from "./hooks/useDarkMode";

const Page = () => {
  const { settings } = useSettingsAtom();
  const { toggle } = useDarkMode();

  useSettingsModalListener();
  useErrorModalListener();
  const { hardwareInfo } = useHardwareInfoAtom();
  console.log(hardwareInfo);

  useEffect(() => {
    if (settings?.theme) {
      toggle(settings.theme === "dark");
    }
  }, [settings?.theme, toggle]);

  return (
    <div className="bg-slate-200 dark:bg-gray-900 text-gray-900 dark:text-white">
      <h1>Hardware Monitor Proto</h1>
      <ChartTemplate />
      <SettingsSheet />
    </div>
  );
};

export default Page;
