import {
  cpuUsageHistoryAtom,
  graphicUsageHistoryAtom,
  memoryUsageHistoryAtom,
} from "@/atom/chart";
import { useSettingsAtom } from "@/atom/useSettingsAtom";
import LineChart from "@/components/charts/LineChart";
import { chartConfig } from "@/consts/chart";
import { useUsageUpdater } from "@/hooks/useHardwareData";

import { useAtom } from "jotai";
import { useMemo } from "react";

const labels = Array(chartConfig.historyLengthSec).fill("");

const CpuUsageChart = () => {
  const [cpuUsageHistory] = useAtom(cpuUsageHistoryAtom);
  useUsageUpdater("cpu");

  return (
    <LineChart labels={labels} chartData={cpuUsageHistory} dataType="cpu" />
  );
};

const MemoryUsageChart = () => {
  const [memoryUsageHistory] = useAtom(memoryUsageHistoryAtom);
  useUsageUpdater("memory");

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
  useUsageUpdater("gpu");

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
