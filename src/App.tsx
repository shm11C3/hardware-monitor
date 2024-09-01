import { useEffect, useState } from "react";
import TestTemplate from "./components/Sample";
import ChartTemplate from "./template/Chart";
import "./index.css";
import { useSettingsModalListener } from "@/hooks/useTauriEventListener";
import { useSettingsAtom } from "./atom/useSettingsAtom";

import SettingsSheet from "@/template/SettingsSheet";
import { useDarkMode } from "./hooks/useDarkMode";

type ButtonState = "chart" | "raw";

const Page = () => {
	const [buttonState, setButtonState] = useState<ButtonState>("chart");
	const { settings } = useSettingsAtom();
	const { toggle } = useDarkMode();

	useSettingsModalListener();

	const handleShowData = () => {
		setButtonState(buttonState === "raw" ? "chart" : "raw");
	};

	useEffect(() => {
		if (settings?.theme) {
			toggle(settings.theme === "dark");
		}
	}, [settings?.theme, toggle]);

	return (
		<div className="bg-slate-200 dark:bg-gray-900 text-gray-900 dark:text-white">
			<h1>Hardware Monitor Proto</h1>
			<div>
				<button type="button" onClick={handleShowData}>
					{buttonState === "chart" ? "Show Raw Data" : "Show Chart Sample"}
				</button>
			</div>
			{buttonState === "raw" ? <TestTemplate /> : <ChartTemplate />}
			<SettingsSheet />
		</div>
	);
};

export default Page;
