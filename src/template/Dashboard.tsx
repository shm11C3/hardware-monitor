import {
  cpuUsageHistoryAtom,
  graphicUsageHistoryAtom,
  memoryUsageHistoryAtom,
} from "@/atom/chart";
import { useHardwareInfoAtom } from "@/atom/useHardwareInfoAtom";
import DoughnutChart from "@/components/charts/DoughnutChart";
import { useAtom } from "jotai";

const InfoTable = ({
  title,
  data,
}: {
  title?: string;
  data: { [key: string]: string | number };
}) => {
  return (
    <div className="p-4 border rounded-md shadow-md bg-gray-800 text-white">
      {title && <h4 className="text-xl font-bold mb-2">{title}</h4>}
      <table className="w-full text-left">
        <tbody>
          {Object.keys(data).map((key) => (
            <tr key={key} className="border-b border-gray-700">
              <th className="pr-4 py-2 text-gray-400">{key}</th>
              <td className="py-2">{data[key]}</td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
};

const DataArea = ({ children }: { children: React.ReactNode }) => {
  return (
    <div className="p-4">
      <div className="border rounded-2xl border-zinc-600">
        <div className="p-4">{children}</div>
      </div>
    </div>
  );
};

const CPUInfo = () => {
  const [cpuUsageHistory] = useAtom(cpuUsageHistoryAtom);
  const { hardwareInfo } = useHardwareInfoAtom();

  return (
    hardwareInfo.cpu && (
      <>
        <DoughnutChart
          chartData={cpuUsageHistory[cpuUsageHistory.length - 1]}
          dataType={"usage"}
          hardType="cpu"
        />
        <InfoTable
          data={{
            Name: hardwareInfo.cpu.name,
            Vendor: hardwareInfo.cpu.vendor,
            "Core Count": hardwareInfo.cpu.coreCount,
            "Default Clock Speed": `${hardwareInfo.cpu.clock} ${hardwareInfo.cpu.clockUnit}`,
          }}
        />
      </>
    )
  );
};

const GPUInfo = () => {
  const [graphicUsageHistory] = useAtom(graphicUsageHistoryAtom);
  const { hardwareInfo } = useHardwareInfoAtom();

  return (
    hardwareInfo.gpus && (
      <>
        <DoughnutChart
          chartData={graphicUsageHistory[graphicUsageHistory.length - 1]}
          dataType={"usage"}
          hardType="gpu"
        />
        <InfoTable
          data={{
            Name: hardwareInfo.gpus[0].name,
            Vendor: hardwareInfo.gpus[0].vendorName,
            "Memory Size": hardwareInfo.gpus[0].memorySize,
            "Memory Size Dedicated": hardwareInfo.gpus[0].memorySizeDedicated,
          }}
        />
      </>
    )
  );
};

const MemoryInfo = () => {
  const [memoryUsageHistory] = useAtom(memoryUsageHistoryAtom);
  const { hardwareInfo } = useHardwareInfoAtom();

  return (
    hardwareInfo.memory && (
      <>
        <DoughnutChart
          chartData={memoryUsageHistory[memoryUsageHistory.length - 1]}
          dataType={"usage"}
          hardType="memory"
        />
        <InfoTable
          data={{
            "Memory Type": hardwareInfo.memory.memoryType,
            "Total Memory": hardwareInfo.memory.size,
            "Memory Count": `${hardwareInfo.memory.memoryCount}/${hardwareInfo.memory.totalSlots}`,
            "Memory Clock": `${hardwareInfo.memory.clock} ${hardwareInfo.memory.clockUnit}`,
          }}
        />
      </>
    )
  );
};

const Dashboard = () => {
  return (
    <div className="w-3/4 mx-auto p-4">
      <div className="grid grid-cols-1 md:grid-cols-2 gap-4 ">
        <DataArea>
          <CPUInfo />
        </DataArea>
        <DataArea>
          <GPUInfo />
        </DataArea>
      </div>

      <DataArea>
        <MemoryInfo />
      </DataArea>
    </div>
  );
};

export default Dashboard;
