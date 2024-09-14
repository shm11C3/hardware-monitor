export type ChartDataType = "cpu" | "memory" | "gpu";

export type CpuInfo = {
  name: string;
  clock: number;
  clockUnit: string;
  vendor: string;
  coreCount: number;
  frequency: number;
  cpuName: string;
};

export type MemoryInfo = {
  size: string;
  clock: number;
  clockUnit: string;
  memoryCount: number;
  memoryType: string;
};

export type HardwareInfo = {
  cpu: CpuInfo;
  memory: MemoryInfo;
};
