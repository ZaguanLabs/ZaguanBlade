import i18n from 'i18next';
import { initReactI18next } from 'react-i18next';
import LanguageDetector from 'i18next-browser-languagedetector';
import HttpBackend from 'i18next-http-backend';

export const supportedAppLanguages = ['en', 'es'] as const;
export type AppLanguage = (typeof supportedAppLanguages)[number];

export function normalizeAppLanguage(language?: string | null): AppLanguage {
  const normalized = language?.trim().toLowerCase();
  if (!normalized) {
    return 'en';
  }

  const baseLanguage = normalized.split('-')[0];
  return baseLanguage === 'es' ? 'es' : 'en';
}

function syncDocumentLanguage(language?: string | null) {
  if (typeof document === 'undefined') {
    return;
  }

  document.documentElement.lang = normalizeAppLanguage(language);
}

i18n
  .use(HttpBackend)
  .use(LanguageDetector)
  .use(initReactI18next)
  .init({
    fallbackLng: 'en',
    supportedLngs: supportedAppLanguages,
    load: 'languageOnly', // Load 'en' instead of 'en-US'
    debug: import.meta.env.DEV,
    detection: {
      order: ['localStorage', 'navigator', 'htmlTag'],
      caches: ['localStorage'],
    },

    interpolation: {
      escapeValue: false, // React already escapes output — double-escaping breaks content
    },

    backend: {
      loadPath: '/locales/{{lng}}/{{ns}}.json',
    }
  });

i18n.on('languageChanged', (language) => {
  syncDocumentLanguage(language);
});

syncDocumentLanguage(i18n.resolvedLanguage || i18n.language);

export default i18n;
