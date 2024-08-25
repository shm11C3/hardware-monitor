import { useEffect, useState } from "react";
import TestTemplate from "./components/Sample";
import ChartTemplate from "./template/Chart";
import "./index.css";
import { atom, useAtom } from "jotai";
import { useDarkMode } from "./hooks/useDarkMode";
import { getSettings } from "./services/settingService";
import type { Settings } from "./types/settingsType";

type ButtonState = "chart" | "raw";

const settingsAtom = atom<Settings | null>(null);

const useLoadSettings = () => {
	const [, setSettings] = useAtom(settingsAtom);

	useEffect(() => {
		const loadSettings = async () => {
			const setting = await getSettings();
			setSettings(setting);
		};
		loadSettings();
	}, [setSettings]);
};

const Page = () => {
	const [buttonState, setButtonState] = useState<ButtonState>("chart");
	const [settings] = useAtom(settingsAtom);
	const { toggle } = useDarkMode();

	useLoadSettings();

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
		</div>
	);
};

export default Page;
