import {
	getCpuMemoryHistory,
	getCpuUsage,
	getCpuUsageHistory,
	getGpuUsage,
	getGpuUsageHistory,
	getMemoryUsage,
} from "@/services/hardwareService";
import { useEffect, useState } from "react";

const Sample = () => {
	const [cpuUsage, setCpuUsage] = useState(0);
	const [memoryUsage, setMemoryUsage] = useState(0);
	const [gpuUsage, setGpuUsage] = useState(0);

	const [cpuHistory, setCpuHistory] = useState<number[]>([]);
	const [memoryHistory, setMemoryHistory] = useState<number[]>([]);
	const [gpuHistory, setGpuHistory] = useState<number[]>([]);

	useEffect(() => {
		const interval = setInterval(async () => {
			setCpuUsage(await getCpuUsage());
			setMemoryUsage(await getMemoryUsage());
			setCpuHistory(await getCpuUsageHistory(30));
			setMemoryHistory(await getCpuMemoryHistory(30));
			setGpuUsage(await getGpuUsage());
			setGpuHistory(await getGpuUsageHistory(30));
		}, 1000);

		return () => clearInterval(interval);
	}, []);

	return (
		<div>
			<p>CPU: {cpuUsage}%</p>
			<p>MEMORY: {memoryUsage}%</p>
			<p>GPU: {gpuUsage}%</p>

			<div>
				<h3>CPU History</h3>
				<p>Count: {cpuHistory.length}</p>
				<ul>
					{cpuHistory.map((item, index) => (
						<li key={index.toString()}>{item}</li>
					))}
				</ul>
				<h3>MEMORY History</h3>
				<p>Count: {memoryHistory.length}</p>
				<ul>
					{memoryHistory.map((item, index) => (
						<li key={index.toString()}>{item}</li>
					))}
				</ul>
				<h3>GPU History</h3>
				<p>Count: {gpuHistory.length}</p>
				<ul>
					{gpuHistory.map((item, index) => (
						<li key={index.toString()}>{item}</li>
					))}
				</ul>
			</div>
		</div>
	);
};

export default Sample;
