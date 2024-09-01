import { listen } from "@tauri-apps/api/event";
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

export const useSettingsModalListener = () => {
	const setShowSettingsModal = useSetAtom(modalAtoms.showSettingsModal);

	// モーダルを開くイベントをリッスン
	useTauriEventListener("open_settings", () => {
		setShowSettingsModal(true);
	});

	const closeModal = () => setShowSettingsModal(false);

	return { closeModal };
};
