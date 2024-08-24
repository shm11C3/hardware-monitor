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
import type { ChartData } from "chart.js";
import { Line } from "react-chartjs-2";
import type { ChartDataType } from "../types/chartType";

ChartJS.register(
	CategoryScale,
	LinearScale,
	PointElement,
	LineElement,
	Title,
	Tooltip,
	Legend,
);

const LineChart = ({
	labels,
	chartData,
	dataType,
}: {
	labels: string[];
	chartData: number[];
	dataType: ChartDataType;
}) => {
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

	const data: Record<ChartDataType, ChartData<"line", number[], string>> = {
		cpu: {
			labels,
			datasets: [
				{
					label: "CPU Usage (%)",
					data: chartData,
					borderColor: "rgb(75, 192, 192)",
					backgroundColor: "rgba(75, 192, 192, 0.3)",
					fill: true,
				},
			],
		},
		memory: {
			labels,
			datasets: [
				{
					label: "Memory Usage (%)",
					data: chartData,
					borderColor: "rgb(255, 99, 132)",
					backgroundColor: "rgba(255, 99, 132, 0.3)",
					fill: true,
				},
			],
		},
		gpu: {
			labels,
			datasets: [
				{
					label: "GPU Usage (%)",
					data: chartData,
					borderColor: "rgb(255, 206, 86)",
					backgroundColor: "rgba(255, 206, 86, 0.3)",
					fill: true,
				},
			],
		},
	};

	return (
		<div className="chart-container">
			<Line data={data[dataType]} options={options} />
		</div>
	);
};

export default LineChart;
