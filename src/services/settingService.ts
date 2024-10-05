import type { Settings } from "@/types/settingsType";
import { invoke } from "@tauri-apps/api/tauri";

export const getSettings = async (): Promise<Settings> => {
  return await invoke("get_settings");
};

export const setTheme = async (theme: Settings["theme"]): Promise<void> => {
  return await invoke("set_theme", { newTheme: theme });
};

export const setDisplayTargets = async (
  targets: Settings["display_targets"],
): Promise<void> => {
  return await invoke("set_display_targets", { newTargets: targets });
};

export const setGraphSize = async (
  size: Settings["graphSize"],
): Promise<void> => {
  return await invoke("set_graph_size", { newSize: size });
};

export const setLanguage = async (value: string): Promise<void> => {
  return await invoke("set_language", { newLanguage: value });
};
