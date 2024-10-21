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
import { Switch } from "@/components/ui/switch";
import { sizeOptions } from "@/consts/chart";
import type { ChartDataType } from "@/types/hardwareDataType";
import type { Settings as SettingTypes } from "@/types/settingsType";

const SettingGraphType = () => {
  const { settings, toggleDisplayTarget } = useSettingsAtom();
  const selectedGraphTypes = settings.displayTargets;

  const toggleGraphType = async (type: ChartDataType) => {
    await toggleDisplayTarget(type);
  };

  return (
    <div className="flex items-center space-x-4 py-6">
      <Label htmlFor="graphType" className="text-lg self-start">
        Hardware Type
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
    <div className="flex items-center space-x-4 py-6">
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
    <div className="py-6 w-full">
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

const SettingGraphSwitch = ({
  label,
  type,
}: {
  label: string;
  type:
    | "lineGraphBorder"
    | "lineGraphFill"
    | "lineGraphMix"
    | "lineGraphShowLegend"
    | "lineGraphShowScale";
}) => {
  const { settings, updateSettingAtom } = useSettingsAtom();

  const SettingGraphSwitch = async (value: boolean) => {
    await updateSettingAtom(type, value);
  };

  return (
    <div className="space-y-6">
      <div className="flex items-center space-x-4 py-3">
        <div className="w-full flex flex-row items-center justify-between rounded-lg border p-4">
          <div className="space-y-0.5">
            <Label htmlFor={type} className="text-lg">
              {label}
            </Label>
          </div>

          <Switch
            checked={settings[type]}
            onCheckedChange={SettingGraphSwitch}
          />
        </div>
      </div>
    </div>
  );
};

const SettingColorInput = ({
  label,
  hardwareType,
}: { label: string; hardwareType: ChartDataType }) => {
  const { settings, updateLineGraphColorAtom } = useSettingsAtom();

  const updateGraphColor = async (value: string) => {
    await updateLineGraphColorAtom(hardwareType, value);
  };

  // カンマ区切りのRGB値を16進数に変換
  const hexValue = `#${settings.lineGraphColor[hardwareType]
    .split(",")
    .map((value) => Number(value).toString(16))
    .join("")}`;

  return (
    <div className="grid grid-cols-5 gap-4 py-6">
      <Label htmlFor={hardwareType} className="text-lg">
        {label}
      </Label>

      <input
        type="color"
        className="w-6 h-6 border-none p-0 cursor-pointer"
        value={hexValue}
        onChange={(e) => updateGraphColor(e.target.value)}
      />
    </div>
  );
};

const Settings = () => {
  return (
    <>
      <div className="my-8 p-4">
        <h3 className="text-2xl font-bold py-3">General</h3>
        <SettingColorMode />
        <SettingGraphType />
      </div>
      <div className="my-8 p-4">
        <h3 className="text-2xl font-bold py-3">Custom Theme</h3>
        <div className="grid md:grid-cols-1 lg:grid-cols-3 gap-6">
          {/** TODO 横にグラフのプレビューを出したい */}
          <div className="w-4/5">
            <SettingGraphSwitch
              label="Line Chart Border"
              type="lineGraphBorder"
            />
            <SettingGraphSwitch label="Line Chart Fill" type="lineGraphFill" />
            <SettingGraphSwitch label="Line Chart Mix" type="lineGraphMix" />
            <SettingGraphSwitch
              label="Line Chart Legend"
              type="lineGraphShowLegend"
            />
            <SettingGraphSwitch
              label="Line Chart Show Scale"
              type="lineGraphShowScale"
            />
            <SettingLineChartSize />
          </div>
          <div className="w-4/5">
            <h4 className="text-xl font-bold">Line Color</h4>
            <SettingColorInput label="CPU" hardwareType="cpu" />
            <SettingColorInput label="RAM" hardwareType="memory" />
            <SettingColorInput label="GPU" hardwareType="gpu" />
          </div>
        </div>
      </div>
    </>
  );
};

export default Settings;
