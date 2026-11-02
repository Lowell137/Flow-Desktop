import { useAppSettingsStore } from "../../store/useAppSettingsStore";
import { SETTINGS } from "../settings/schema";

/** Tabs are on unless turned off (Settings > Content); off, Flow works as a single page per window. */
export function tabsEnabled(): boolean {
  return useAppSettingsStore.getState().values[SETTINGS.TABS_ENABLED] !== "false";
}

export function useTabsEnabled(): boolean {
  return useAppSettingsStore((state) => state.values[SETTINGS.TABS_ENABLED] !== "false");
}
