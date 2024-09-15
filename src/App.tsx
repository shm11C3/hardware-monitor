import { useEffect } from "react";
import ChartTemplate from "./template/Chart";
import "./index.css";
import {
  useErrorModalListener,
  useSettingsModalListener,
} from "@/hooks/useTauriEventListener";
import SettingsSheet from "@/template/SettingsSheet";
import { useAtom } from "jotai";
import { selectedMenuAtom } from "./atom/ui";
import { useHardwareInfoAtom } from "./atom/useHardwareInfoAtom";
import { useSettingsAtom } from "./atom/useSettingsAtom";
import { useDarkMode } from "./hooks/useDarkMode";
import SideMenu from "./template/SideMenu";
import type { SelectedMenuType } from "./types/ui";

const Page = () => {
  const { settings } = useSettingsAtom();
  const [selectedMenu] = useAtom(selectedMenuAtom);
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

  const displayTargets: Record<SelectedMenuType, JSX.Element> = {
    dashboard: <div>TODO</div>,
    usage: <ChartTemplate />,
    settings: <SettingsSheet />,
  };

  return (
    <div className="bg-slate-200 dark:bg-gray-900 text-gray-900 dark:text-white">
      <SideMenu />
      {displayTargets[selectedMenu]}
      <SettingsSheet />
    </div>
  );
};

export default Page;
