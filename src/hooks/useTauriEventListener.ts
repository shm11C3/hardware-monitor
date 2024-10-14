import { listen } from "@tauri-apps/api/event";
import { message } from "@tauri-apps/plugin-dialog";
import { useSetAtom } from "jotai";
import { useEffect } from "react";
import { modalAtoms } from "../atom/ui";

const useTauriEventListener = (event: string, callback: () => void) => {
  useEffect(() => {
    const unListen = listen(event, callback);

    return () => {
      unListen.then((unListen) => unListen());
    };
  }, [event, callback]);
};

/**
 * モーダルを開くイベントをリッスンして、モーダルを表示する
 *
 * @returns closeModal
 */
export const useSettingsModalListener = () => {
  const setShowSettingsModal = useSetAtom(modalAtoms.showSettingsModal);

  // モーダルを開くイベントをリッスン
  useTauriEventListener("open_settings", () => {
    setShowSettingsModal(true);
  });

  const closeModal = () => setShowSettingsModal(false);

  return { closeModal };
};

/**
 * バックエンド側のエラーイベントをリッスンして、エラーダイヤログを表示する
 */
export const useErrorModalListener = () => {
  useEffect(() => {
    const unListen = listen("error_event", (event) => {
      const { title, message: errorMessage } = event.payload as {
        title: string;
        message: string;
      };

      message(errorMessage, {
        title: title,
        kind: "error",
      });
    });

    return () => {
      unListen.then((off) => off());
    };
  }, []);
};
