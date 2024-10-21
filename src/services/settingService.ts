import type { Settings } from "@/types/settingsType";
import { invoke } from "@tauri-apps/api/core";

export const getSettings = async (): Promise<Settings> => {
  return await invoke("get_settings");
};

export const setTheme = async (theme: Settings["theme"]): Promise<void> => {
  return await invoke("set_theme", { newTheme: theme });
};

export const setDisplayTargets = async (
  targets: Settings["displayTargets"],
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

export const setLineGraphBorder = async (value: boolean): Promise<void> => {
  return await invoke("set_line_graph_border", { newValue: value });
};

export const setLineGraphFill = async (value: boolean): Promise<void> => {
  return await invoke("set_line_graph_fill", { newValue: value });
};

export const setLineGraphColor = async <
  K extends keyof Settings["lineGraphColor"],
>(
  key: K,
  value: Settings["lineGraphColor"][K],
): Promise<string> => {
  return await invoke<string>("set_line_graph_color", {
    target: key,
    newColor: value,
  });
};

export const setLineGraphMix = async (value: boolean): Promise<void> => {
  return await invoke("set_line_graph_mix", { newValue: value });
};

export const setLineGraphShowLegend = async (value: boolean): Promise<void> => {
  return await invoke("set_line_graph_show_legend", { newValue: value });
};

export const setLineGraphShowScale = async (value: boolean): Promise<void> => {
  return await invoke("set_line_graph_show_scale", { newValue: value });
};

export const setState = async <K extends keyof Settings["state"]>(
  key: K,
  value: Settings["state"][K],
): Promise<void> => {
  return await invoke("set_state", { key: key, newValue: value });
};
