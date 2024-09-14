import type { ChartDataType } from "./hardwareDataType";

export type Settings = {
  language: string;
  theme: "light" | "dark";
  display_targets: Array<ChartDataType>;
};
