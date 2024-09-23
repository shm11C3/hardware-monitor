import type { Temperatures } from "@/types/hardwareDataType";
import { atom } from "jotai";

export const cpuUsageHistoryAtom = atom<number[]>([]);
export const memoryUsageHistoryAtom = atom<number[]>([]);
export const graphicUsageHistoryAtom = atom<number[]>([]);
export const cpuTempAtom = atom<Temperatures>([]);
export const gpuTempAtom = atom<Temperatures>([]);
