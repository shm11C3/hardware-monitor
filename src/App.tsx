import { useState } from "react";
import Chart from "./components/LineChart";
import TestTemplate from "./components/Sample";

type ButtonState = "chart" | "raw";

const Page = () => {
	const [buttonState, setButtonState] = useState<ButtonState>("chart");

	const handleShowData = () => {
		setButtonState(buttonState === "raw" ? "chart" : "raw");
	};

	return (
		<div>
			<h1>Hardware Monitor Proto</h1>
			<div>
				<button type="button" onClick={handleShowData}>
					{buttonState === "chart" ? "Show Raw Data" : "Show Chart Sample"}
				</button>
			</div>
			{buttonState === "raw" ? <TestTemplate /> : <Chart />}
		</div>
	);
};

export default Page;
