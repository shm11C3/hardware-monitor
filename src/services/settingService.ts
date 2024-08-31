import type { Settings } from "@/types/settingsType";
import { invoke } from "@tauri-apps/api/tauri";

export const getSettings = async (): Promise<Settings> => {
	return await invoke("get_settings");
};
