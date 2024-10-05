import { useSettingsAtom } from "@/atom/useSettingsAtom";
import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Slider } from "@/components/ui/slider";
import { sizeOptions } from "@/consts/chart";
import type { ChartDataType } from "@/types/hardwareDataType";
import type { Settings as SettingTypes } from "@/types/settingsType";

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
  const { settings, updateSettingAtom } = useSettingsAtom();

  const toggleDarkMode = async (mode: "light" | "dark") => {
    await updateSettingAtom("theme", mode);
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
  const { settings, updateSettingAtom } = useSettingsAtom();
  const sizeIndex = sizeOptions.indexOf(
    settings.graphSize as SettingTypes["graphSize"],
  );

  const changeGraphSize = async (value: number[]) => {
    await updateSettingAtom("graphSize", sizeOptions[value[0]]);
  };

  return (
    <div className="my-4 w-96 ">
      <Label className="block text-lg mb-2">Line Chart Size</Label>
      <Slider
        min={0}
        max={sizeOptions.length - 1}
        step={1}
        value={[sizeIndex]}
        onValueChange={changeGraphSize}
        className="w-full mt-4"
      />
      <div className="flex justify-between text-sm mt-2">
        {sizeOptions.map((size) => (
          <span key={size}>{size.toUpperCase()}</span>
        ))}
      </div>
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
