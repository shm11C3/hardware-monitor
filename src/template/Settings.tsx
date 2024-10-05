import { useSettingsAtom } from "@/atom/useSettingsAtom";
import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { sizes } from "@/consts/chart";
import type { ChartDataType } from "@/types/hardwareDataType";

const SettingGraphType = () => {
  const { settings, toggleDisplayTarget } = useSettingsAtom();
  const selectedGraphTypes = settings.display_targets;

  const toggleGraphType = async (type: ChartDataType) => {
    await toggleDisplayTarget(type);
  };

  return (
    <div className="flex items-center space-x-4 py-4">
      <Label htmlFor="graphType" className="text-lg self-start">
        Graph Types
      </Label>
      <div>
        <label className="flex items-center space-x-2">
          <input
            type="checkbox"
            checked={selectedGraphTypes.includes("cpu")}
            onChange={() => toggleGraphType("cpu")}
          />
          <span>CPU</span>
        </label>
        <label className="flex items-center space-x-2">
          <input
            type="checkbox"
            checked={selectedGraphTypes.includes("memory")}
            onChange={() => toggleGraphType("memory")}
          />
          <span>Memory</span>
        </label>
        <label className="flex items-center space-x-2">
          <input
            type="checkbox"
            checked={selectedGraphTypes.includes("gpu")}
            onChange={() => toggleGraphType("gpu")}
          />
          <span>GPU</span>
        </label>
      </div>
    </div>
  );
};

const SettingColorMode = () => {
  const { settings, toggleTheme } = useSettingsAtom();

  const toggleDarkMode = async (mode: "light" | "dark") => {
    await toggleTheme(mode);
  };

  return (
    <div className="flex items-center space-x-4 py-4">
      <Label htmlFor="darkMode" className="text-lg">
        Color Mode
      </Label>
      <Select value={settings.theme} onValueChange={toggleDarkMode}>
        <SelectTrigger className="w-[180px]">
          <SelectValue placeholder="Theme" />
        </SelectTrigger>
        <SelectContent>
          <SelectItem value="light">Light</SelectItem>
          <SelectItem value="dark">Dark</SelectItem>
        </SelectContent>
      </Select>
    </div>
  );
};

const SettingLineChartSize = () => {
  const { settings, toggleGraphSize } = useSettingsAtom();

  return (
    <div className="flex items-center space-x-4 py-4">
      <Label className="text-lg">Line Chart Size</Label>
      <Select value={settings.graphSize} onValueChange={toggleGraphSize}>
        <SelectTrigger className="w-[180px]">
          <SelectValue placeholder="Please select" />
        </SelectTrigger>
        <SelectContent>
          {sizes.map((size) => (
            <SelectItem key={size} value={size}>
              {size.toUpperCase()}
            </SelectItem>
          ))}
        </SelectContent>
      </Select>
    </div>
  );
};

const Settings = () => {
  return (
    <>
      <SettingColorMode />
      <SettingGraphType />
      <SettingLineChartSize />
    </>
  );
};

export default Settings;
