import { invoke } from "@tauri-apps/api/tauri";
import type { Settings } from "../types/settingsType";

export const getSettings = async (): Promise<Settings> => {
	return await invoke("get_settings");
};
