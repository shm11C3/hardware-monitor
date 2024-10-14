import type {
  HardwareInfo,
  NameValues,
  ProcessInfo,
} from "@/types/hardwareDataType";
import { invoke } from "@tauri-apps/api/core";

export const getProcesses = async (): Promise<ProcessInfo[]> => {
  return await invoke("get_process_list");
};

export const getCpuUsage = async (): Promise<number> => {
  return await invoke("get_cpu_usage");
};

export const getHardwareInfo = async (): Promise<
  Exclude<HardwareInfo, "isFetched">
> => {
  return await invoke("get_hardware_info");
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

export const getGpuTemperature = async (): Promise<NameValues> => {
  return await invoke("get_gpu_temperature");
};

export const getGpuFanSpeed = async (): Promise<NameValues> => {
  return await invoke("get_nvidia_gpu_cooler");
};
