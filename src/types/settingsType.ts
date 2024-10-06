import type { sizeOptions } from "@/consts/chart";
import type { ChartDataType } from "./hardwareDataType";
import type { SelectedDisplayType } from "./ui";

export type Settings = {
  language: string;
  theme: "light" | "dark";
  displayTargets: Array<ChartDataType>;
  graphSize: (typeof sizeOptions)[number];
  state: {
    display: SelectedDisplayType;
  };
};
