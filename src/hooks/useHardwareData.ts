import {
  cpuFanSpeedAtom,
  cpuTempAtom,
  cpuUsageHistoryAtom,
  gpuFanSpeedAtom,
  gpuTempAtom,
  graphicUsageHistoryAtom,
  memoryUsageHistoryAtom,
} from "@/atom/chart";
import { chartConfig } from "@/consts/chart";
import {
  getCpuUsage,
  getGpuFanSpeed,
  getGpuTemperature,
  getGpuUsage,
  getMemoryUsage,
} from "@/services/hardwareService";
import type { ChartDataType, NameValues } from "@/types/hardwareDataType";
import { type PrimitiveAtom, useSetAtom } from "jotai";
import { useEffect } from "react";

/**
 * ハードウェア使用率の履歴を更新する
 */
export const useUsageUpdater = (dataType: ChartDataType) => {
  type AtomActionMapping = {
    atom: PrimitiveAtom<number[]>;
    action: () => Promise<number>;
  };

  const mapping: Record<ChartDataType, AtomActionMapping> = {
    cpu: {
      atom: cpuUsageHistoryAtom,
      action: getCpuUsage,
    },
    memory: {
      atom: memoryUsageHistoryAtom,
      action: getMemoryUsage,
    },
    gpu: {
      atom: graphicUsageHistoryAtom,
      action: getGpuUsage,
    },
  };

  const setHistory = useSetAtom(mapping[dataType].atom);
  const getUsage = mapping[dataType].action;

  useEffect(() => {
    const intervalId = setInterval(async () => {
      const usage = await getUsage();
      setHistory((prev) => {
        const newHistory = [...prev, usage];

        // 履歴保持数に満たない場合は0で埋める
        const paddedHistory = Array(
          Math.max(chartConfig.historyLengthSec - newHistory.length, 0),
        )
          .fill(null)
          .concat(newHistory);
        return paddedHistory.slice(-chartConfig.historyLengthSec);
      });
    }, 1000);

    return () => clearInterval(intervalId);
  }, [setHistory, getUsage]);
};

export const useHardwareUpdater = (
  hardType: Exclude<ChartDataType, "memory">,
  dataType: "temp" | "fan",
) => {
  type AtomActionMapping = {
    atom: PrimitiveAtom<NameValues>;
    action: () => Promise<NameValues>;
  };

  const mapping: Record<
    Exclude<ChartDataType, "memory">,
    Record<"temp" | "fan", AtomActionMapping>
  > = {
    cpu: {
      temp: {
        atom: cpuTempAtom,
        action: () => {
          console.error("Not implemented");
          return Promise.resolve([]);
        },
      },
      fan: {
        atom: cpuFanSpeedAtom,
        action: () => {
          console.error("Not implemented");
          return Promise.resolve([]);
        },
      },
    },
    gpu: {
      fan: {
        atom: gpuFanSpeedAtom,
        action: getGpuFanSpeed,
      },
      temp: {
        atom: gpuTempAtom,
        action: getGpuTemperature,
      },
    },
  };

  const setData = useSetAtom(mapping[hardType][dataType].atom);
  const getData = mapping[hardType][dataType].action;

  useEffect(() => {
    const fetchData = async () => {
      const temp = await getData();
      setData(temp);
    };

    fetchData();

    const intervalId = setInterval(async () => {
      fetchData;
    }, 10000);

    return () => clearInterval(intervalId);
  }, [setData, getData]);
};
