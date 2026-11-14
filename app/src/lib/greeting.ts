import { t } from "./i18n.svelte";

/** Home's heading for the local hour (0–23), in the interface language. */
export function greeting(hour: number): string {
  if (hour >= 5 && hour < 12) return t("greeting.morning");
  if (hour >= 12 && hour < 17) return t("greeting.afternoon");
  if (hour >= 17 && hour < 21) return t("greeting.evening");
  return t("greeting.night");
}
