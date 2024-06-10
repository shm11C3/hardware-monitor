import { invoke } from "@tauri-apps/api/tauri";

export const getCpuUsage = async (): Promise<number> => {
	return await invoke("get_cpu_usage");
};

export const getMemoryUsage = async (): Promise<number> => {
	return await invoke("get_memory_usage");
};

export const getCpuUsageHistory = (seconds: number): Promise<number[]> => {
	return invoke("get_cpu_usage_history", { seconds: seconds });
};

export const getCpuMemoryHistory = (seconds: number): Promise<number[]> => {
	return invoke("get_memory_usage_history", { seconds: seconds });
};

export const getGpuUsage = async (): Promise<number> => {
	return await invoke("get_gpu_usage");
};

export const getGpuUsageHistory = (seconds: number): Promise<number[]> => {
	return invoke("get_gpu_usage_history", { seconds: seconds });
};
