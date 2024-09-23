export type ChartDataType = "cpu" | "memory" | "gpu";

export type HardwareDataType = "temp" | "usage" | "clock";

export type CpuInfo = {
  name: string;
  clock: number;
  clockUnit: string;
  vendor: string;
  coreCount: number;
  cpuName: string;
};

export type MemoryInfo = {
  size: string;
  clock: number;
  clockUnit: string;
  memoryCount: number;
  totalSlots: number;
  memoryType: string;
};

export type GraphicInfo = {
  clock: number;
  name: string;
  vendorName: string;
  memorySize: string;
  memorySizeDedicated: string;
};

export type HardwareInfo = {
  cpu?: CpuInfo;
  memory?: MemoryInfo;
  gpus: GraphicInfo[];
  isFetched: boolean;
};

export type Temperatures = Array<{
  name: string;
  value: number;
}>;
