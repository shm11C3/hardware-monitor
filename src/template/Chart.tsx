import { useEffect, useState } from "react";
import LineChart from "../components/LineChart";
import {
	getCpuMemoryHistory,
	getCpuUsageHistory,
	getGpuUsageHistory,
} from "../services/hardwareService";

const ChartTemplate = () => {
	const [cpuData, setCpuData] = useState<number[]>([]);
	const [memoryData, setMemoryData] = useState<number[]>([]);
	const [gpuData, setGpuData] = useState<number[]>([]);
	const [labels, setLabels] = useState<string[]>([]);

	useEffect(() => {
		const fetchData = async () => {
			const seconds = 60;
			const newCpuData = await getCpuUsageHistory(seconds);
			const newMemoryData = await getCpuMemoryHistory(seconds);
			const newGpuData = await getGpuUsageHistory(seconds);

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
		};

		const interval = setInterval(fetchData, 1000);
		return () => clearInterval(interval);
	}, [cpuData, memoryData, gpuData, labels]);

	return (
		<div className="chart-container">
			<LineChart labels={labels} chartData={cpuData} dataType="cpu" />
			<LineChart labels={labels} chartData={memoryData} dataType="memory" />
			<LineChart labels={labels} chartData={gpuData} dataType="gpu" />
		</div>
	);
};

export default ChartTemplate;
