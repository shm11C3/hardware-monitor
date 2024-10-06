import type { SelectedDisplayType } from "@/types/ui";
import { atom } from "jotai";

export const modalAtoms = {
  showSettingsModal: atom<boolean>(false),
};
