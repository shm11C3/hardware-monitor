import { settingsAtom } from "@/atom/main";
import { modalAtoms } from "@/atom/ui";
import { Button } from "@/components/ui/button";
import { Label } from "@/components/ui/label";
import {
	Sheet,
	SheetClose,
	SheetContent,
	SheetDescription,
	SheetFooter,
	SheetHeader,
	SheetTitle,
} from "@/components/ui/sheet";
import { useSettingsModalListener } from "@/hooks/useTauriEventListener";
import type { ChartDataType } from "@/types/chartType";
import { useAtom } from "jotai";

const SettingGraphType = () => {
	const [settings] = useAtom(settingsAtom);
	const selectedGraphTypes = settings.display_targets;

	const toggleGraphType = (type: ChartDataType) => {
		console.log(type);
	};

	return (
		<div className="grid grid-cols-4 items-center gap-4">
			<Label htmlFor="graphType" className="text-right">
				Graph Types
			</Label>
			<div className="col-span-3 space-y-2">
				<label className="flex items-center space-x-2">
					<input
						type="checkbox"
						checked={selectedGraphTypes.includes("cpu")}
						onChange={() => toggleGraphType("cpu")}
					/>
					<span>Line Graph</span>
				</label>
				<label className="flex items-center space-x-2">
					<input
						type="checkbox"
						checked={selectedGraphTypes.includes("memory")}
						onChange={() => toggleGraphType("memory")}
					/>
					<span>Bar Graph</span>
				</label>
				<label className="flex items-center space-x-2">
					<input
						type="checkbox"
						checked={selectedGraphTypes.includes("gpu")}
						onChange={() => toggleGraphType("gpu")}
					/>
					<span>Pie Chart</span>
				</label>
			</div>
		</div>
	);
};

const SettingColorMode = () => {
	const [settings] = useAtom(settingsAtom);

	const toggleDarkMode = (mode: "light" | "dark") => {
		console.log(mode);
	};

	return (
		<div className="grid grid-cols-4 items-center gap-4">
			<Label htmlFor="darkMode" className="text-right">
				Dark Mode
			</Label>
			<div className="col-span-3 flex space-x-2">
				<Button
					variant={settings.theme === "light" ? "default" : "secondary"}
					onClick={() => toggleDarkMode("light")}
				>
					Light Mode
				</Button>
				<Button
					variant={settings.theme === "dark" ? "default" : "secondary"}
					onClick={() => toggleDarkMode("dark")}
				>
					Dark Mode
				</Button>
			</div>
		</div>
	);
};

const SettingsSheet = () => {
	const [showSettingsModal] = useAtom(modalAtoms.showSettingsModal);
	const { closeModal } = useSettingsModalListener();

	return (
		<Sheet open={showSettingsModal} onOpenChange={closeModal}>
			<SheetContent>
				<SheetHeader>
					<SheetTitle>Edit Preference</SheetTitle>
					<SheetDescription>
						Make changes to your preferences here. Click save when you're done.
					</SheetDescription>
				</SheetHeader>
				<div className="grid gap-4 py-4">
					<SettingColorMode />
					<SettingGraphType />
				</div>
				<SheetFooter>
					<SheetClose asChild>
						<Button type="submit">Save changes</Button>
					</SheetClose>
				</SheetFooter>
			</SheetContent>
		</Sheet>
	);
};

export default SettingsSheet;
