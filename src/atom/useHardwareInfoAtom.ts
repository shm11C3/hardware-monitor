import { getHardwareInfo } from "@/services/hardwareService";
import type { HardwareInfo } from "@/types/hardwareDataType";
import { atom, useAtom } from "jotai";
import { useEffect } from "react";

const hardInfoAtom = atom<HardwareInfo>({
  isFetched: false,
});

export const useHardwareInfoAtom = () => {
  const [hardwareInfo, setHardInfo] = useAtom(hardInfoAtom);

  useEffect(() => {
    if (!hardwareInfo.isFetched) {
      const init = async () => {
        try {
          const fetchedHardwareInfo = await getHardwareInfo();

          setHardInfo({
            ...fetchedHardwareInfo,
            isFetched: true,
          });
        } catch (e) {
          console.error(e);
        }
      };

      init();
    }
  }, [hardwareInfo.isFetched, setHardInfo]);

  return { hardwareInfo };
};
