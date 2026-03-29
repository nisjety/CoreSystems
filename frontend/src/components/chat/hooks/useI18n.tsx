"use client";
// i18n hook + provider. Moved from .ts to .tsx to allow JSX.
import React, { useState, useEffect, useCallback, createContext, useContext, useMemo } from 'react';

export type Locale = 'en' | 'no';
type TranslationMap = Record<string, string>;

const baseTranslations: Record<Locale, TranslationMap> = {
	en: {
		'chat.placeholder.default': 'What would you like to know?',
		'chat.placeholder.thinking': 'AI is thinking',
		'chat.placeholder.variant1': 'Ask me anything...',
		'chat.placeholder.variant2': 'How can I help you today?',
		'chat.placeholder.variant3': "What's on your mind?",
		'chat.placeholder.variant4': 'Need assistance with something?',
		'chat.placeholder.variant5': 'Ready to chat?',
		'chat.button.send': 'Send',
		'chat.button.newChat': 'New Chat',
		'chat.status.online': 'Online',
		'chat.status.typing': 'is typing...',
		'chat.welcome.title': 'Welcome to Aquatiq Chat',
		'chat.welcome.description': 'Your intelligent AI assistant',
		'chat.empty.title': 'Start a conversation',
		'chat.empty.description': "Ask me anything or describe what you'd like to create.",
		'chat.action.scrollBottom': 'Scroll to bottom',
		'chat.action.copy': 'Copy',
		'chat.action.regenerate': 'Regenerate',
		'chat.action.share': 'Share',
		'error.network': 'Network error occurred',
		'error.generic': 'An error occurred',
		'common.retry': 'Retry',
		'common.updated': 'Updated',
		'common.refresh': 'Refresh',
		'dashboard.greetings.morning': 'Good morning',
		'dashboard.greetings.afternoon': 'Good afternoon',
		'dashboard.greetings.evening': 'Good evening',
		'dashboard.subtitle': 'Your productivity & insights hub',
		'dashboard.chatPlaceholder': 'Ask Aquatiq anything or describe a task...',
		'dashboard.welcomeTitle': 'Welcome to Aquatiq',
		'dashboard.welcomeText': 'Start your first AI-powered conversation to explore insights and speed up your workflow.',
		'dashboard.startFirstChat': 'Start first chat',
		'dashboard.exploreFeatures': 'Explore powerful features',
		'dashboard.features.ai': 'AI Assistance',
		'dashboard.features.fast': 'Fast Responses',
		'dashboard.features.search': 'Smart Search',
		'dashboard.features.creative': 'Creative Ideas',
		'weather.title': 'Weather',
		'weather.location': 'Oslo, Norway',
		'weather.useMyLocation': 'Use my location',
		'weather.unavailable': 'Weather unavailable',
		'weather.wind': 'Wind',
		'weather.humidity': 'Humidity',
		'weather.precipitation': 'Precipitation',
		'weather.tomorrow': 'Tomorrow',
		'weather.nextDays': 'Next days',
		'weather.conditions.clear': 'Clear',
		'weather.conditions.partlyCloudy': 'Partly cloudy',
		'weather.conditions.overcast': 'Overcast',
		'weather.conditions.cloudy': 'Cloudy',
		'weather.conditions.heavyRain': 'Heavy rain',
		'weather.conditions.lightRain': 'Light rain',
		'weather.conditions.rain': 'Rain',
		'weather.conditions.heavySnow': 'Heavy snow',
		'weather.conditions.lightSnow': 'Light snow',
		'weather.conditions.snow': 'Snow',
		'weather.conditions.sleet': 'Sleet',
		'weather.conditions.thunderstorm': 'Thunderstorm',
		'weather.conditions.fog': 'Fog',
		'weather.conditions.mist': 'Mist',
		'weather.conditions.unknown': 'Unknown',
		'news.title': 'Latest News',
		'news.unavailable': 'News unavailable',
		'news.filter': 'Filter',
		'news.showFewer': 'Show fewer',
		'news.showAll': 'Show all ({{count}})',
		'news.articles': 'articles',
		'news.justNow': 'just now',
		'news.categories.all': 'All',
		'news.categories.general': 'General',
		'news.categories.technology': 'Technology',
		'news.categories.business': 'Business',
		'news.categories.sports': 'Sports',
		'traffic.title': 'Traffic',
		'traffic.unavailable': 'Traffic data unavailable',
		'traffic.loadingLocation': 'Loading location...',
		'traffic.accurateLocation': 'Get precise location',
		'traffic.searchPlaceholder': 'Search traffic points...',
		'traffic.noData': 'No traffic points found',
		'traffic.pointsFound': '{{count}} points',
		'traffic.volume': 'Volume',
		'traffic.speed': 'Speed',
		'traffic.distance': 'Distance',
		'traffic.status.operational': 'Operational',
		'traffic.status.maintenance': 'Maintenance',
		'traffic.status.offline': 'Offline',
		'traffic.showFewer': 'Show fewer',
		'traffic.showAll': 'Show all',
		'aquatiq.title': 'Aquatiq Updates',
		'aquatiq.unavailable': 'Aquatiq updates unavailable',
		'aquatiq.learnMore': 'Learn more',
		'aquatiq.priorityShort': 'Priority {{priority}}',
		'aquatiq.offers': 'offers',
		'aquatiq.viewAll': 'View all',
		'aquatiq.categories.product': 'Product',
		'aquatiq.categories.event': 'Event',
		'aquatiq.categories.business': 'Business'
	},
	no: {
		'chat.placeholder.default': 'Hva vil du vite?',
		'chat.placeholder.thinking': 'AI tenker',
		'chat.placeholder.variant1': 'Spør meg om hva som helst...',
		'chat.placeholder.variant2': 'Hvordan kan jeg hjelpe deg i dag?',
		'chat.placeholder.variant3': 'Hva tenker du på?',
		'chat.placeholder.variant4': 'Trenger du hjelp med noe?',
		'chat.placeholder.variant5': 'Klar til å chatte?',
		'chat.button.send': 'Send',
		'chat.button.newChat': 'Ny chat',
		'chat.status.online': 'Pålogget',
		'chat.status.typing': 'skriver...',
		'chat.welcome.title': 'Velkommen til Aquatiq Chat',
		'chat.welcome.description': 'Din intelligente AI-assistent',
		'chat.empty.title': 'Start en samtale',
		'chat.empty.description': 'Spør meg om hva som helst eller beskriv hva du vil lage.',
		'chat.action.scrollBottom': 'Rull til bunnen',
		'chat.action.copy': 'Kopier',
		'chat.action.regenerate': 'Regenerer',
		'chat.action.share': 'Del',
		'error.network': 'Nettverksfeil oppstod',
		'error.generic': 'En feil oppstod',
		'common.retry': 'Prøv igjen',
		'common.updated': 'Oppdatert',
		'common.refresh': 'Oppdater',
		'dashboard.greetings.morning': 'God morgen',
		'dashboard.greetings.afternoon': 'God ettermiddag',
		'dashboard.greetings.evening': 'God kveld',
		'dashboard.subtitle': 'Din produktivitets- og innsiktshub',
		'dashboard.chatPlaceholder': 'Spør Aquatiq om hva som helst eller beskriv en oppgave...',
		'dashboard.welcomeTitle': 'Velkommen til Aquatiq',
		'dashboard.welcomeText': 'Start din første AI-drevne samtale for å utforske innsikt og effektivisere arbeidet.',
		'dashboard.startFirstChat': 'Start første chat',
		'dashboard.exploreFeatures': 'Utforsk kraftige funksjoner',
		'dashboard.features.ai': 'AI-assistanse',
		'dashboard.features.fast': 'Raske svar',
		'dashboard.features.search': 'Smart søk',
		'dashboard.features.creative': 'Kreative ideer',
		'weather.title': 'Vær',
		'weather.location': 'Oslo, Norge',
		'weather.useMyLocation': 'Bruk min posisjon',
		'weather.unavailable': 'Vær utilgjengelig',
		'weather.wind': 'Vind',
		'weather.humidity': 'Fuktighet',
		'weather.precipitation': 'Nedbør',
		'weather.tomorrow': 'I morgen',
		'weather.nextDays': 'Neste dager',
		'weather.conditions.clear': 'Klart',
		'weather.conditions.partlyCloudy': 'Delvis skyet',
		'weather.conditions.overcast': 'Overskyet',
		'weather.conditions.cloudy': 'Skyet',
		'weather.conditions.heavyRain': 'Kraftig regn',
		'weather.conditions.lightRain': 'Lett regn',
		'weather.conditions.rain': 'Regn',
		'weather.conditions.heavySnow': 'Kraftig snø',
		'weather.conditions.lightSnow': 'Lett snø',
		'weather.conditions.snow': 'Snø',
		'weather.conditions.sleet': 'Sludd',
		'weather.conditions.thunderstorm': 'Tordenvær',
		'weather.conditions.fog': 'Tåke',
		'weather.conditions.mist': 'Dis',
		'weather.conditions.unknown': 'Ukjent',
		'news.title': 'Siste nyheter',
		'news.unavailable': 'Nyheter utilgjengelig',
		'news.filter': 'Filter',
		'news.showFewer': 'Vis færre',
		'news.showAll': 'Vis alle ({{count}})',
		'news.articles': 'artikler',
		'news.justNow': 'akkurat nå',
		'news.categories.all': 'Alle',
		'news.categories.general': 'Generelt',
		'news.categories.technology': 'Teknologi',
		'news.categories.business': 'Business',
		'news.categories.sports': 'Sport',
		'traffic.title': 'Trafikk',
		'traffic.unavailable': 'Trafikkdata utilgjengelig',
		'traffic.loadingLocation': 'Laster posisjon...',
		'traffic.accurateLocation': 'Hent nøyaktig posisjon',
		'traffic.searchPlaceholder': 'Søk trafikkpunkter...',
		'traffic.noData': 'Ingen trafikkpunkter funnet',
		'traffic.pointsFound': '{{count}} punkter',
		'traffic.volume': 'Volum',
		'traffic.speed': 'Hastighet',
		'traffic.distance': 'Avstand',
		'traffic.status.operational': 'Operativ',
		'traffic.status.maintenance': 'Vedlikehold',
		'traffic.status.offline': 'Frakoblet',
		'traffic.showFewer': 'Vis færre',
		'traffic.showAll': 'Vis alle',
		'aquatiq.title': 'Aquatiq Oppdateringer',
		'aquatiq.unavailable': 'Aquatiq oppdateringer utilgjengelig',
		'aquatiq.learnMore': 'Les mer',
		'aquatiq.priorityShort': 'Prioritet {{priority}}',
		'aquatiq.offers': 'tilbud',
		'aquatiq.viewAll': 'Se alle',
		'aquatiq.categories.product': 'Produkt',
		'aquatiq.categories.event': 'Arrangement',
		'aquatiq.categories.business': 'Business'
	}
};

interface I18nContextValue {
	locale: Locale;
	t: (key: string, vars?: Record<string, string | number>) => string;
	changeLocale: (l: Locale) => void;
	getPlaceholders: () => string[];
	interpolate: (template: string, vars?: Record<string, string | number>) => string;
}

const I18nAppContext = createContext<I18nContextValue | undefined>(undefined);

export const AppI18nProvider: React.FC<{ children: React.ReactNode; initialLocale?: Locale }> = ({ children, initialLocale = 'en' }) => {
	const [locale, setLocale] = useState<Locale>(initialLocale);

	useEffect(() => {
		if (typeof window !== 'undefined') {
			const savedLocale = localStorage.getItem('aquatiq-locale') as Locale | null;
			if (savedLocale && translations[savedLocale]) {
				setLocale(savedLocale);
			}
		}
	}, []);

	const changeLocale = useCallback((newLocale: Locale) => {
		if (!translations[newLocale]) return;
		setLocale(newLocale);
		if (typeof window !== 'undefined') {
			localStorage.setItem('aquatiq-locale', newLocale);
		}
	}, []);

	const interpolate = useCallback((template: string, vars?: Record<string, string | number>) => {
		if (!vars) return template;
		return Object.entries(vars).reduce((acc, [k, v]) => acc.replace(new RegExp(`{{${k}}}`, 'g'), String(v)), template);
	}, []);

	const t = useCallback((key: string, vars?: Record<string, string | number>) => {
		const dict = baseTranslations[locale] || {};
		const template = dict[key] ?? key;
		return interpolate(template, vars);
	}, [locale, interpolate]);

	const getPlaceholders = useCallback(() => ([
		t('chat.placeholder.default'),
		t('chat.placeholder.variant1'),
		t('chat.placeholder.variant2'),
		t('chat.placeholder.variant3'),
		t('chat.placeholder.variant4'),
		t('chat.placeholder.variant5'),
	]), [t]);

	const value = useMemo<I18nContextValue>(() => ({ locale, t, changeLocale, getPlaceholders, interpolate }), [locale, t, changeLocale, getPlaceholders, interpolate]);

	return <I18nAppContext.Provider value={value}>{children}</I18nAppContext.Provider>;
};

export const useI18n = (_initialLocale?: Locale) => {
	const ctx = useContext(I18nAppContext);
	const [fallbackLocale, setFallbackLocale] = useState<Locale>(_initialLocale || 'en');
	const fbInterpolate = useCallback((template: string, vars?: Record<string, string | number>) => {
		if (!vars) return template;
		return Object.entries(vars).reduce((acc, [k, v]) => acc.replace(new RegExp(`{{${k}}}`, 'g'), String(v)), template);
	}, []);
	const fbT = useCallback((key: string, vars?: Record<string, string | number>) => {
		const dict = baseTranslations[fallbackLocale] || {};
		const template = dict[key] ?? key;
		return fbInterpolate(template, vars);
	}, [fallbackLocale, fbInterpolate]);
	const fbChange = useCallback((l: Locale) => setFallbackLocale(l), []);
	const fbPlaceholders = useCallback(() => ([
		fbT('chat.placeholder.default'),
		fbT('chat.placeholder.variant1'),
		fbT('chat.placeholder.variant2'),
		fbT('chat.placeholder.variant3'),
		fbT('chat.placeholder.variant4'),
		fbT('chat.placeholder.variant5'),
	]), [fbT]);

	if (ctx) return ctx;
	return { locale: fallbackLocale, t: fbT, changeLocale: fbChange, getPlaceholders: fbPlaceholders, interpolate: fbInterpolate } as I18nContextValue;
};

export const translations = baseTranslations;
export type { TranslationMap };
