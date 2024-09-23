import { getHardwareInfo } from "@/services/hardwareService";
import type { HardwareInfo } from "@/types/hardwareDataType";
import { atom, useAtom } from "jotai";
import { useEffect } from "react";

const hardInfoAtom = atom<HardwareInfo>({
  isFetched: false,
  gpus: [],
});

export const useHardwareInfoAtom = () => {
  const [hardwareInfo, setHardInfo] = useAtom(hardInfoAtom);

  useEffect(() => {
    const init = async () => {
      try {
        const hardwareInfo = await getHardwareInfo();
        setHardInfo(hardwareInfo);

        hardwareInfo.isFetched = true;
      } catch (e) {
        console.error(e);
        hardwareInfo.isFetched = false;
      }
    };

    // データがなければ取得して更新
    if (!hardwareInfo.isFetched) {
      init();
    }
  }, [setHardInfo, hardwareInfo]);

  return { hardwareInfo };
};
