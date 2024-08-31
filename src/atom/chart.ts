import { atom } from "jotai";

export const cpuUsageHistoryAtom = atom<number[]>([]);
export const memoryUsageHistoryAtom = atom<number[]>([]);
export const graphicUsageHistoryAtom = atom<number[]>([]);
