import { useState } from "react";
import TestTemplate from "./components/Sample";
import ChartTemplate from "./template/Chart";
import "./index.css";

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
			{buttonState === "raw" ? <TestTemplate /> : <ChartTemplate />}
		</div>
	);
};

export default Page;
