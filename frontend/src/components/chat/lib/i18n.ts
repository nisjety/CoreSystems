import i18n from 'i18next';
import { initReactI18next } from 'react-i18next';
import LanguageDetector from 'i18next-browser-languagedetector';

// Import translation files
import enCommon from '../../../../public/locales/en/common.json';
import noCommon from '../../../../public/locales/no/common.json';

const resources = {
  en: {
    common: enCommon,
  },
  no: {
    common: noCommon,
  },
};

i18n
  .use(LanguageDetector)
  .use(initReactI18next)
  .init({
    resources,
    fallbackLng: 'en',
    supportedLngs: ['en', 'no'],
    detection: {
      // order and from where user language should be detected
      order: ['querystring', 'localStorage', 'navigator', 'htmlTag'],
      caches: ['localStorage'],
    },
    interpolation: {
      escapeValue: false,
    },
    defaultNS: 'common',
    ns: ['common'],
  });

export default i18n;
