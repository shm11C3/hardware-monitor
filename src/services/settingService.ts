import { invoke } from "@tauri-apps/api/tauri";

export const getColorTheme = async (): Promise<string> => {
	return await invoke("get_windows_theme_mode");
};
