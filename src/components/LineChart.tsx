import {
	CategoryScale,
	Chart as ChartJS,
	type ChartOptions,
	Legend,
	LineElement,
	LinearScale,
	PointElement,
	Title,
	Tooltip,
} from "chart.js";
import { useEffect, useState } from "react";
import { Line } from "react-chartjs-2";
import {
	getCpuMemoryHistory,
	getCpuUsageHistory,
	getGpuUsageHistory,
} from "../services/hardwareService";

ChartJS.register(
	CategoryScale,
	LinearScale,
	PointElement,
	LineElement,
	Title,
	Tooltip,
	Legend,
);

const LineChart = () => {
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

	const options: ChartOptions<"line"> = {
		responsive: true,
		animation: false,
		scales: {
			x: { display: false },
			y: {
				display: true,
				suggestedMin: 0,
				suggestedMax: 100,
				grid: { color: "rgba(255, 255, 255, 0.2)" },
				ticks: { color: "#fff" },
			},
		},
		elements: {
			point: { radius: 0, hoverRadius: 0 },
			line: { tension: 0.4 },
		},
		plugins: {
			legend: { display: true, labels: { color: "#fff" } },
			tooltip: {
				backgroundColor: "rgba(0, 0, 0, 0.7)",
				titleColor: "#fff",
				bodyColor: "#fff",
			},
		},
	};

	const cpuChartData = {
		labels,
		datasets: [
			{
				label: "CPU Usage (%)",
				data: cpuData,
				borderColor: "rgb(75, 192, 192)",
				backgroundColor: "rgba(75, 192, 192, 0.3)",
				fill: true,
			},
		],
	};

	const memoryChartData = {
		labels,
		datasets: [
			{
				label: "Memory Usage (%)",
				data: memoryData,
				borderColor: "rgb(255, 99, 132)",
				backgroundColor: "rgba(255, 99, 132, 0.3)",
				fill: true,
			},
		],
	};

	const gpuChartData = {
		labels,
		datasets: [
			{
				label: "GPU Usage (%)",
				data: gpuData,
				borderColor: "rgb(255, 206, 86)",
				backgroundColor: "rgba(255, 206, 86, 0.3)",
				fill: true,
			},
		],
	};

	return (
		<div className="chart-container">
			<Line data={cpuChartData} options={options} />
			<Line data={memoryChartData} options={options} />
			<Line data={gpuChartData} options={options} />
		</div>
	);
};

export default LineChart;
