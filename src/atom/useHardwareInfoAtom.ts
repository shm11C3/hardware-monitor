import { getHardwareInfo } from "@/services/hardwareService";
import type { HardwareInfo } from "@/types/hardwareDataType";
import { atom, useAtom } from "jotai";
import { useEffect } from "react";

const hardInfoAtom = atom<HardwareInfo | null>();

export const useHardwareInfoAtom = () => {
  const [hardware, setHardInfo] = useAtom(hardInfoAtom);

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
    if (!hardware) {
      init();
    }

    console.log(hardware);
  }, [setHardInfo, hardware]);

  return { hardware };
};
