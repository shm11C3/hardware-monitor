import {
  cpuUsageHistoryAtom,
  graphicUsageHistoryAtom,
  memoryUsageHistoryAtom,
} from "@/atom/chart";
import { useSettingsAtom } from "@/atom/useSettingsAtom";
import LineChart from "@/components/charts/LineChart";
import { chartConfig } from "@/consts/chart";
import { useAtom } from "jotai";
import { useMemo } from "react";

const labels = Array(chartConfig.historyLengthSec).fill("");

const CpuUsageChart = () => {
  const [cpuUsageHistory] = useAtom(cpuUsageHistoryAtom);

  return (
    <LineChart labels={labels} chartData={cpuUsageHistory} dataType="cpu" />
  );
};

const MemoryUsageChart = () => {
  const [memoryUsageHistory] = useAtom(memoryUsageHistoryAtom);

  return (
    <LineChart
      labels={labels}
      chartData={memoryUsageHistory}
      dataType="memory"
    />
  );
};

const GpuUsageChart = () => {
  const [graphicUsageHistory] = useAtom(graphicUsageHistoryAtom);

  return (
    <LineChart labels={labels} chartData={graphicUsageHistory} dataType="gpu" />
  );
};

const ChartTemplate = () => {
  const { settings } = useSettingsAtom();

  const renderedCharts = useMemo(() => {
    return (
      <>
        {settings?.display_targets.includes("cpu") && <CpuUsageChart />}
        {settings?.display_targets.includes("memory") && <MemoryUsageChart />}
        {settings?.display_targets.includes("gpu") && <GpuUsageChart />}
      </>
    );
  }, [settings]);

  return <div className="p-8">{renderedCharts}</div>;
};

export default ChartTemplate;
