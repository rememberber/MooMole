import { useSyncExternalStore } from 'react'

export const languages = ['zh', 'en', 'ja'] as const
export type Language = (typeof languages)[number]
export type LanguagePreference = Language | 'system'
export const localeTags: Record<Language, string> = { zh: 'zh-CN', en: 'en-US', ja: 'ja-JP' }

// Resolve the primary system language. A later preferred language must not
// override the English fallback for an unsupported primary language.
export function resolveLanguage(preference: unknown, systemLanguage: string): Language {
  if (languages.includes(preference as Language)) return preference as Language
  const primary = systemLanguage.trim().toLowerCase().split(/[-_]/)[0]
  return languages.includes(primary as Language) ? (primary as Language) : 'en'
}
let nativeSystemLanguage: string | undefined
const systemLanguage = () =>
  nativeSystemLanguage ?? (typeof navigator === 'undefined' ? 'en' : navigator.language)
export function setSystemLanguage(value: string | undefined) {
  nativeSystemLanguage = value
}
let preference: LanguagePreference = 'system'
let language = resolveLanguage(preference, systemLanguage())
const listeners = new Set<() => void>()
export const getLanguage = () => language
export const getLocale = () => localeTags[language]
export function setLanguagePreference(value: LanguagePreference = 'system') {
  preference = value
  const next = resolveLanguage(value, systemLanguage())
  if (language === next) return
  language = next
  listeners.forEach((listener) => listener())
}
export function useLanguage() {
  return useSyncExternalStore(
    (listener) => {
      listeners.add(listener)
      return () => listeners.delete(listener)
    },
    getLanguage,
    () => 'en' as Language,
  )
}
if (typeof window !== 'undefined')
  window.addEventListener('languagechange', () => setLanguagePreference(preference))
