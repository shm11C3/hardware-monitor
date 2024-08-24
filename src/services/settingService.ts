import { invoke } from "@tauri-apps/api/tauri";

export const getSettings = async (): Promise<string> => {
	return await invoke("get_settings");
};
