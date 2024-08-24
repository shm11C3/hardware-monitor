import { useEffect, useState } from "react";
import TestTemplate from "./components/Sample";
import ChartTemplate from "./template/Chart";
import "./index.css";
import { useDarkMode } from "./hooks/useDarkMode";
import { getSettings } from "./services/settingService";
import type { Settings } from "./types/settingsType";

type ButtonState = "chart" | "raw";

const useLoadSettings = () => {
	const [settingState, setSettingState] = useState<Settings | null>(null);

	useEffect(() => {
		const loadSettings = async () => {
			const setting = await getSettings();
			setSettingState(setting);
		};
		loadSettings();
	}, []);

	return settingState;
};

const Page = () => {
	const [buttonState, setButtonState] = useState<ButtonState>("chart");
	const settingState = useLoadSettings();
	const { toggle } = useDarkMode();

	const handleShowData = () => {
		setButtonState(buttonState === "raw" ? "chart" : "raw");
	};

	useEffect(() => {
		if (settingState?.theme) {
			toggle(settingState.theme === "dark");
		}
	}, [settingState?.theme, toggle]);

	return (
		<div className="bg-slate-200 dark:bg-gray-900 text-gray-900 dark:text-white">
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
