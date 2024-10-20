import type { sizeOptions } from "@/consts/chart";
import type { ChartDataType } from "./hardwareDataType";
import type { SelectedDisplayType } from "./ui";

export type Settings = {
  language: string;
  theme: "light" | "dark";
  displayTargets: Array<ChartDataType>;
  graphSize: (typeof sizeOptions)[number];
  lineGraphBorder: boolean;
  lineGraphFill: boolean;
  lineGraphColor: {
    cpu: string;
    memory: string;
    gpu: string;
  };
  lineGraphMix: boolean;
  lineGraphShowLegend: boolean;
  lineGraphShowScale: boolean;
  state: {
    display: SelectedDisplayType;
  };
};
