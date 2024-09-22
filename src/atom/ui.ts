import type { SelectedMenuType } from "@/types/ui";
import { atom } from "jotai";

export const modalAtoms = {
  showSettingsModal: atom<boolean>(false),
};

export const selectedMenuAtom = atom<SelectedMenuType>("usage"); // [TODO] change to dashboard
