import { createContext, useContext, useState, useCallback, type ReactNode } from 'react';
import en, { type Translations } from './en';
import zh from './zh';

type Lang = 'en' | 'zh';

type I18nContextValue = {
  lang: Lang;
  t: Translations;
  setLang: (lang: Lang) => void;
};

const translations: Record<Lang, Translations> = { en, zh };

const STORAGE_KEY = 'rustproxy-lang';

const getDefaultLang = (): Lang => {
  try {
    const stored = localStorage.getItem(STORAGE_KEY);
    if (stored === 'en' || stored === 'zh') return stored;
  } catch {
    // localStorage unavailable
  }
  const browser = navigator.language.toLowerCase();
  return browser.startsWith('zh') ? 'zh' : 'en';
};

const I18nContext = createContext<I18nContextValue | null>(null);

export function I18nProvider({ children }: { children: ReactNode }) {
  const [lang, setLangState] = useState<Lang>(getDefaultLang);

  const setLang = useCallback((next: Lang) => {
    setLangState(next);
    try {
      localStorage.setItem(STORAGE_KEY, next);
    } catch {
      // localStorage unavailable
    }
  }, []);

  const value: I18nContextValue = {
    lang,
    t: translations[lang],
    setLang,
  };

  return (
    <I18nContext.Provider value={value}>
      {children}
    </I18nContext.Provider>
  );
}

export function useI18n(): I18nContextValue {
  const ctx = useContext(I18nContext);
  if (!ctx) throw new Error('useI18n must be used inside I18nProvider');
  return ctx;
}

export function LangToggle() {
  const { lang, setLang, t } = useI18n();

  const toggle = () => {
    setLang(lang === 'en' ? 'zh' : 'en');
  };

  return (
    <button
      type="button"
      className="btn-ghost btn-sm lang-toggle"
      onClick={toggle}
      aria-label={t.lang.toggle}
      title={t.lang.toggle}
    >
      {lang === 'en' ? '中文' : 'EN'}
    </button>
  );
}
