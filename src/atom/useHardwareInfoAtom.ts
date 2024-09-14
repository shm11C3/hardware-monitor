import { getHardwareInfo } from "@/services/hardwareService";
import type { HardwareInfo } from "@/types/hardwareDataType";
import { atom, useAtom } from "jotai";
import { useEffect } from "react";

const hardInfoAtom = atom<HardwareInfo | null>();

export const useHardwareInfoAtom = () => {
  const [hardwareInfo, setHardInfo] = useAtom(hardInfoAtom);

  useEffect(() => {
    const init = async () => {
      try {
        const hardwareInfo = await getHardwareInfo();
        setHardInfo(hardwareInfo);
      } catch (e) {
        console.error(e);
      }
    };

    // データがなければ取得して更新
    if (!hardwareInfo) {
      init();
    }
  }, [setHardInfo, hardwareInfo]);

  return { hardwareInfo };
};
