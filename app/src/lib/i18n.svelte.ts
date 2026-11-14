// Interface language: the catalogs, the active locale and `t`, which reads it. Components call
// `t` in their markup or in `$derived`, so a language change redraws them at once.

import type { LanguageSetting } from "./bindings";
import { en, type Message, type MessageKey } from "./locales/en";
import { vi } from "./locales/vi";

export type Locale = "en" | "vi";
export type { MessageKey };

const catalogs: Record<Locale, Record<MessageKey, Message>> = { en, vi };

/** The first of the system's preferred languages that Sonder has, else English. */
export function systemLocale(languages: readonly string[] = browserLanguages()): Locale {
  for (const tag of languages) {
    const primary = tag.toLowerCase().split(/[-_]/)[0];
    if (primary === "vi" || primary === "en") return primary;
  }
  return "en";
}

function browserLanguages(): readonly string[] {
  if (typeof navigator === "undefined") return [];
  return navigator.languages?.length ? navigator.languages : [navigator.language];
}

/** The locale a language setting stands for; "system" (or none yet) follows the OS. */
export function resolveLocale(setting: LanguageSetting | null | undefined, languages?: readonly string[]): Locale {
  return setting === "en" || setting === "vi" ? setting : systemLocale(languages);
}

class I18n {
  locale = $state<Locale>(systemLocale());
}

export const i18n = new I18n();

/** Switches the interface to the language `setting` names. */
export function applyLanguage(setting: LanguageSetting | null | undefined) {
  i18n.locale = resolveLocale(setting);
  if (typeof document !== "undefined") document.documentElement.lang = i18n.locale;
}

export type Params = Record<string, string | number>;

/** Plural rules and number format per locale, built once: `t` runs for every row of a list. */
const formats = new Map<Locale, { plural: Intl.PluralRules; number: Intl.NumberFormat }>();
function formatsFor(locale: Locale) {
  let found = formats.get(locale);
  if (!found) {
    found = { plural: new Intl.PluralRules(locale), number: new Intl.NumberFormat(locale) };
    formats.set(locale, found);
  }
  return found;
}

/**
 * The message `key` in the active language, with `{name}` placeholders filled from `params`.
 * A plural message picks its form by `params.count`.
 */
export function t(key: MessageKey, params?: Params): string {
  const locale = i18n.locale;
  const message = catalogs[locale][key] ?? en[key];
  let text: string;
  if (typeof message === "string") {
    text = message;
  } else {
    const form = formatsFor(locale).plural.select(Number(params?.count ?? 0));
    text = form === "one" ? message.one : message.other;
  }
  if (!params) return text;
  // Numbers read in the locale's own style ("1,234" / "1.234"); preformatted strings stay as given.
  const { number } = formatsFor(locale);
  return text.replace(/\{(\w+)\}/g, (whole, name: string) => {
    if (!(name in params)) return whole;
    const value = params[name];
    return typeof value === "number" ? number.format(value) : value;
  });
}
