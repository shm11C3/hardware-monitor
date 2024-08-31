import type { ChartDataType } from "./chartType";

export type Settings = {
	language: string;
	theme: "light" | "dark";
	display_targets: Array<ChartDataType>;
};
