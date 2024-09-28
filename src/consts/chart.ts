import type { ChartDataType, HardwareDataType } from "@/types/hardwareDataType";

export const chartConfig = {
  /**
   * グラフの履歴の長さ（秒）
   */
  historyLengthSec: 60,
} as const;

export const displayHardType: Record<ChartDataType, string> = {
  cpu: "CPU",
  memory: "RAM",
  gpu: "GPU",
} as const;

export const displayDataType: Record<HardwareDataType, string> = {
  temp: "Temperature",
  usage: "Usage",
  clock: "Clock",
} as const;
