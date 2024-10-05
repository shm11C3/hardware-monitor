import { getProcesses } from "@/services/hardwareService";
import type { ProcessInfo } from "@/types/hardwareDataType";
import { CaretDown } from "@phosphor-icons/react";
import { atom, useAtom, useSetAtom } from "jotai";
import { useEffect, useState } from "react";

const processesAtom = atom<ProcessInfo[]>([]);

const ProcessesTable = ({
  defaultItemLength,
}: { defaultItemLength: number }) => {
  const [processes] = useAtom(processesAtom);
  const setAtom = useSetAtom(processesAtom);
  const [showAllItem, setShowAllItem] = useState<boolean>(false);
  const [sortConfig, setSortConfig] = useState<{
    key: keyof ProcessInfo;
    direction: "ascending" | "descending";
  } | null>(null);

  useEffect(() => {
    const fetchProcesses = async () => {
      try {
        const processesData = await getProcesses();
        setAtom(processesData);
      } catch (error) {
        console.error("Failed to fetch processes:", error);
      }
    };

    fetchProcesses();

    const interval = setInterval(fetchProcesses, 3000);

    return () => clearInterval(interval);
  }, [setAtom]);

  const sortedProcesses = [...processes];
  if (sortConfig !== null) {
    sortedProcesses.sort((a, b) => {
      if (a[sortConfig.key] < b[sortConfig.key]) {
        return sortConfig.direction === "ascending" ? -1 : 1;
      }
      if (a[sortConfig.key] > b[sortConfig.key]) {
        return sortConfig.direction === "ascending" ? 1 : -1;
      }
      return 0;
    });
  }

  const requestSort = (key: keyof ProcessInfo) => {
    let direction: "ascending" | "descending" = "ascending";
    if (
      sortConfig &&
      sortConfig.key === key &&
      sortConfig.direction === "ascending"
    ) {
      direction = "descending";
    }
    setSortConfig({ key, direction });
  };

  return (
    <div className="p-4 border rounded-md shadow-md bg-gray-800 text-white">
      <h4 className="text-xl font-bold mb-2">Process</h4>
      <div className="overflow-x-auto">
        <table className="w-full text-left">
          <thead>
            <tr className="border-b border-gray-700">
              <th
                className="pr-4 py-2 text-gray-400 cursor-pointer"
                onClick={() => requestSort("pid")}
                onKeyDown={() => requestSort("pid")}
              >
                PID
              </th>
              <th
                className="pr-4 py-2 text-gray-400 cursor-pointer"
                onClick={() => requestSort("name")}
                onKeyDown={() => requestSort("name")}
              >
                Name
              </th>
              <th
                className="pr-4 py-2 text-gray-400 cursor-pointer"
                onClick={() => requestSort("cpuUsage")}
                onKeyDown={() => requestSort("cpuUsage")}
              >
                CPU Usage
              </th>
              <th
                className="pr-4 py-2 text-gray-400 cursor-pointer"
                onClick={() => requestSort("memoryUsage")}
                onKeyDown={() => requestSort("memoryUsage")}
              >
                Memory Usage
              </th>
            </tr>
          </thead>
          <tbody>
            {sortedProcesses
              .slice(0, showAllItem ? processes.length : defaultItemLength)
              .map((process) => (
                <tr key={process.pid} className="border-b border-gray-700">
                  <td className="py-2">{process.pid}</td>
                  <td className="py-2">{process.name}</td>
                  <td className="py-2">{process.cpuUsage}%</td>
                  <td className="py-2">{process.memoryUsage} MB</td>
                </tr>
              ))}
          </tbody>
        </table>
        {!showAllItem && (
          <button
            type="button"
            onClick={() => setShowAllItem(true)}
            className="w-full flex justify-center items-center py-2 text-gray-400 hover:text-white focus:outline-none mt--4"
          >
            <CaretDown size={32} />
          </button>
        )}
      </div>
    </div>
  );
};

export default ProcessesTable;
