import {
	cpuUsageHistoryAtom,
	graphicUsageHistoryAtom,
	memoryUsageHistoryAtom,
} from "@/atom/chart";
import { chartConfig } from "@/consts/chart";
import {
	getCpuUsage,
	getGpuUsage,
	getMemoryUsage,
} from "@/services/hardwareService";
import type { ChartDataType } from "@/types/hardwareDataType";
import { type PrimitiveAtom, useSetAtom } from "jotai";
import { useEffect } from "react";

type AtomActionMapping = {
	atom: PrimitiveAtom<number[]>;
	action: () => Promise<number>;
};

/**
 * ハードウェア使用率の履歴を更新する
 */
export const useUsageUpdater = (dataType: ChartDataType) => {
	const mapping: Record<ChartDataType, AtomActionMapping> = {
		cpu: {
			atom: cpuUsageHistoryAtom,
			action: getCpuUsage,
		},
		memory: {
			atom: memoryUsageHistoryAtom,
			action: getMemoryUsage,
		},
		gpu: {
			atom: graphicUsageHistoryAtom,
			action: getGpuUsage,
		},
	};

	const setHistory = useSetAtom(mapping[dataType].atom);
	const getUsage = mapping[dataType].action;

	useEffect(() => {
		const intervalId = setInterval(async () => {
			const usage = await getUsage();
			setHistory((prev) => {
				const newHistory = [...prev, usage];

				// 履歴保持数に満たない場合は0で埋める
				const paddedHistory = Array(
					Math.max(chartConfig.historyLengthSec - newHistory.length, 0),
				)
					.fill(null)
					.concat(newHistory);
				return paddedHistory.slice(-chartConfig.historyLengthSec);
			});
		}, 1000);

		return () => clearInterval(intervalId);
	}, [setHistory, getUsage]);
};
