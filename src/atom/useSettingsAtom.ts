import {
	getSettings,
	setDisplayTargets,
	setTheme,
} from "@/services/settingService";
import type { ChartDataType } from "@/types/chartType";
import type { Settings } from "@/types/settingsType";
import { atom, useAtom } from "jotai";
import { useEffect } from "react";
const settingsAtom = atom<Settings>({
	language: "en",
	theme: "light",
	display_targets: [],
});

export const useSettingsAtom = () => {
	const [settings, setSettings] = useAtom(settingsAtom);

	useEffect(() => {
		const loadSettings = async () => {
			const setting = await getSettings();
			setSettings(setting);
		};
		loadSettings();
	}, [setSettings]);

	const toggleDisplayTarget = async (target: ChartDataType) => {
		const newTargets = settings.display_targets.includes(target)
			? settings.display_targets.filter((t) => t !== target)
			: [...settings.display_targets, target];

		setSettings((prev) => ({ ...prev, display_targets: newTargets }));
		await setDisplayTargets(settings.display_targets);
	};

	const toggleTheme = async (theme: "light" | "dark") => {
		setSettings((prev) => ({ ...prev, theme }));
		await setTheme(theme);
	};

	return { settings, toggleDisplayTarget, toggleTheme };
};
