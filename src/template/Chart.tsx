import { settingsAtom } from "@/atom/main";
import LineChart from "@/components/LineChart";
import {
	getCpuMemoryHistory,
	getCpuUsageHistory,
	getGpuUsageHistory,
} from "@/services/hardwareService";
import { useAtom } from "jotai";
import { useCallback, useEffect, useMemo, useState } from "react";

const ChartTemplate = () => {
	const [cpuData, setCpuData] = useState<number[]>([]);
	const [memoryData, setMemoryData] = useState<number[]>([]);
	const [gpuData, setGpuData] = useState<number[]>([]);
	const [labels, setLabels] = useState<string[]>([]);

	const [settings] = useAtom(settingsAtom);

	const fetchData = useCallback(async () => {
		const seconds = 60;

		const newCpuDataPromise = settings?.display_targets.includes("cpu")
			? getCpuUsageHistory(seconds)
			: Promise.resolve([]);
		const newMemoryDataPromise = settings?.display_targets.includes("memory")
			? getCpuMemoryHistory(seconds)
			: Promise.resolve([]);
		const newGpuDataPromise = settings?.display_targets.includes("gpu")
			? getGpuUsageHistory(seconds)
			: Promise.resolve([]);

		const [newCpuData, newMemoryData, newGpuData] = await Promise.all([
			newCpuDataPromise,
			newMemoryDataPromise,
			newGpuDataPromise,
		]);

		if (cpuData.length < seconds) {
			setCpuData([...cpuData, newCpuData[newCpuData.length - 1]]);
			setMemoryData([...memoryData, newMemoryData[newMemoryData.length - 1]]);
			setGpuData([...gpuData, newGpuData[newGpuData.length - 1]]);
			setLabels([...labels, ""]);
		} else {
			setCpuData([...cpuData.slice(1), newCpuData[newCpuData.length - 1]]);
			setMemoryData([
				...memoryData.slice(1),
				newMemoryData[newMemoryData.length - 1],
			]);
			setGpuData([...gpuData.slice(1), newGpuData[newGpuData.length - 1]]);
			setLabels([...labels.slice(1), ""]);
		}
	}, [settings, cpuData, memoryData, gpuData, labels]);

	useEffect(() => {
		const interval = setInterval(fetchData, 1000);
		return () => clearInterval(interval);
	}, [fetchData]);

	const renderedCharts = useMemo(() => {
		return (
			<>
				{settings?.display_targets.includes("cpu") && (
					<LineChart labels={labels} chartData={cpuData} dataType="cpu" />
				)}
				{settings?.display_targets.includes("memory") && (
					<LineChart labels={labels} chartData={memoryData} dataType="memory" />
				)}
				{settings?.display_targets.includes("gpu") && (
					<LineChart labels={labels} chartData={gpuData} dataType="gpu" />
				)}
			</>
		);
	}, [labels, cpuData, memoryData, gpuData, settings]);

	return <div className="chart-container">{renderedCharts}</div>;
};

export default ChartTemplate;
