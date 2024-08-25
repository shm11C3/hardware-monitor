import { atom } from "jotai";
import type { Settings } from "../types/settingsType";

export const settingsAtom = atom<Settings | null>(null);
