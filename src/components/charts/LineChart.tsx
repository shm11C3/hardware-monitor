import { useSettingsAtom } from "@/atom/useSettingsAtom";
import type { ChartDataType } from "@/types/hardwareDataType";
import { Cpu, GraphicsCard, Memory } from "@phosphor-icons/react";
import {
  CategoryScale,
  Chart as ChartJS,
  type ChartOptions,
  Filler,
  Legend,
  LineElement,
  LinearScale,
  PointElement,
  Title,
  Tooltip,
} from "chart.js";
import type { Chart, ChartData } from "chart.js";
import { useRef } from "react";
import { Line } from "react-chartjs-2";
import { tv } from "tailwind-variants";
import CustomLegend, { type LegendItem } from "./CustomLegend";

ChartJS.register(
  CategoryScale,
  LinearScale,
  PointElement,
  LineElement,
  Title,
  Tooltip,
  Legend,
  Filler,
);

const LineChart = ({
  labels,
  chartData,
  dataType,
}: {
  labels: string[];
  chartData: number[];
  dataType: ChartDataType;
}) => {
  const { settings } = useSettingsAtom();

  const chartRef = useRef<Chart<"line">>(null);

  const options: ChartOptions<"line"> = {
    responsive: true,
    animation: false,
    scales: {
      x: { display: false },
      y: {
        display: false,
        suggestedMin: 0,
        suggestedMax: 100,
        grid: { color: "rgba(255, 255, 255, 0.2)" },
        ticks: { color: "#fff" },
      },
    },
    elements: {
      point: { radius: 0, hoverRadius: 0 },
      line: { tension: 0.4 },
    },
    plugins: {
      legend: { display: false },
      tooltip: {
        backgroundColor: "rgba(0, 0, 0, 0.7)",
        titleColor: "#fff",
        bodyColor: "#fff",
      },
    },
  };

  const data: Record<ChartDataType, ChartData<"line", number[], string>> = {
    cpu: {
      labels,
      datasets: [
        {
          label: "CPU Usage (%)",
          data: chartData,
          borderColor: "rgb(75, 192, 192)",
          backgroundColor: "rgba(75, 192, 192, 0.3)",
          fill: true,
        },
      ],
    },
    memory: {
      labels,
      datasets: [
        {
          label: "Memory Usage (%)",
          data: chartData,
          borderColor: "rgb(255, 99, 132)",
          backgroundColor: "rgba(255, 99, 132, 0.3)",
          fill: true,
        },
      ],
    },
    gpu: {
      labels,
      datasets: [
        {
          label: "GPU Usage (%)",
          data: chartData,
          borderColor: "rgb(255, 206, 86)",
          backgroundColor: "rgba(255, 206, 86, 0.3)",
          fill: true,
        },
      ],
    },
  };

  const legendItems: Record<ChartDataType, LegendItem> = {
    cpu: {
      label: "CPU",
      icon: (
        <Cpu size={20} color="rgb(75, 192, 192)" className="text-teal-400" />
      ),
      datasetIndex: 0,
    },
    memory: {
      label: "Memory",
      icon: (
        <Memory size={20} color="rgb(255, 99, 132)" className="text-pink-400" />
      ),
      datasetIndex: 1,
    },
    gpu: {
      label: "GPU",
      icon: (
        <GraphicsCard
          size={20}
          color="rgb(255, 206, 86)"
          className="text-yellow-400"
        />
      ),
      datasetIndex: 2,
    },
  };

  const graphVariants = tv({
    base: "mt-5 mx-auto",
    variants: {
      size: {
        sm: "max-w-screen-sm",
        md: "max-w-screen-md",
        lg: "max-w-screen-lg",
        xl: "max-w-screen-xl",
        "2xl": "max-w-screen-2xl",
      },
    },
    defaultVariants: {
      size: "xl",
    },
  });

  return (
    <div className={graphVariants({ size: settings.graphSize })}>
      <Line
        className="border-2 rounded-xl border-slate-400 dark:border-zinc-600 border-ra "
        ref={chartRef}
        data={data[dataType]}
        options={options}
      />
      <div className="flex justify-center mt-4 mb-2">
        <CustomLegend item={legendItems[dataType]} />
      </div>
    </div>
  );
};

export default LineChart;
