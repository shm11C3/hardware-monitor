import type { sizes } from "@/consts/chart";
import type { ChartDataType } from "./hardwareDataType";

export type Settings = {
  language: string;
  theme: "light" | "dark";
  display_targets: Array<ChartDataType>;
  graphSize: (typeof sizes)[number];
};
