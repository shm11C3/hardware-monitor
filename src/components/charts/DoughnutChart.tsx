import { displayDataType, displayHardType } from "@/consts/chart";
import type { ChartDataType, HardwareDataType } from "@/types/hardwareDataType";
import { Lightning, Speedometer, Thermometer } from "@phosphor-icons/react";
import {
  ArcElement,
  Chart as ChartJS,
  type ChartOptions,
  Legend,
  Tooltip,
} from "chart.js";
import { Doughnut } from "react-chartjs-2";

ChartJS.register(ArcElement, Tooltip, Legend);

const DoughnutChart = ({
  chartData,
  hardType,
  dataType,
}: {
  chartData: number;
  hardType: ChartDataType;
  dataType: HardwareDataType;
}) => {
  const data = {
    datasets: [
      {
        data: [chartData, 100 - chartData],
        backgroundColor: ["#888", "#222"],
        borderWidth: 0,
      },
    ],
  };

  const options: ChartOptions<"doughnut"> = {
    cutout: "85%",
    plugins: {
      tooltip: { enabled: false },
    },
  };

  const dataTypeIcons: Record<HardwareDataType, JSX.Element> = {
    usage: <Lightning className="mr-1" size={18} weight="duotone" />,
    temp: <Thermometer className="mr-1" size={18} weight="duotone" />,
    clock: <Speedometer className="mr-1" size={18} weight="duotone" />,
  };

  return (
    <div className="p-2 w-36 relative">
      <h3 className="text-lg font-bold">{displayHardType[hardType]}</h3>
      <Doughnut data={data} options={options} />
      <div className="absolute inset-0 flex flex-col items-center justify-center">
        <span className="text-white text-xl font-semibold">{chartData}%</span>
      </div>
      <span className="flex justify-center mt-4 text-gray-400">
        {dataTypeIcons[dataType]}
        {displayDataType[dataType]}
      </span>
    </div>
  );
};

export default DoughnutChart;
