import type { Settings } from "@/types/settingsType";
import { atom } from "jotai";

export const settingsAtom = atom<Settings | null>(null);
