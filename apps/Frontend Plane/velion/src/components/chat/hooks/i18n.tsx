"use client";
// hooks/i18n.tsx - Internationalization Hook with Context Provider (implementation)
import React, { useState, useEffect, useCallback, createContext, useContext, useMemo } from 'react';
import { useGlobalLanguage } from '@/components/core/contexts/GlobalLanguageContext';

type Locale = 'en' | 'no';
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
		'chat.placeholder.variant6': 'Tell me what you need...',
		'chat.placeholder.variant7': 'Start a conversation...',
		'chat.placeholder.variant8': 'What can I do for you?',
		'chat.placeholder.variant9': 'How may I assist?',
		'chat.placeholder.variant10': 'Share your thoughts...',
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
		'chat.action.like': 'Good response',
		'chat.action.dislike': 'Poor response',
		'chat.action.copied': 'Copied!',
		'chat.thinking': 'is thinking',
		'chat.justNow': 'just now',
		'chat.timeAgo.minutes': '{{count}}m ago',
		'chat.timeAgo.hours': '{{count}}h ago',
		'chat.timeAgo.days': '{{count}}d ago',
		'chat.models.aquatiqGpt': 'Aquatiq Gpt',
		'chat.models.chat5': 'Chat 5',
		'chat.models.chatBilder': 'GPT Imagen',
		'chat.models.selectModel': 'Select model',
		'chat.models.description.aquatiqGpt': 'Balanced performance',
		'chat.models.description.chat5': 'Enhanced creativity',
		'chat.models.description.chatBilder': 'Generate images',
		'chat.recentChats': 'Recent chats',
		'chat.noRecentChats': 'No recent chats',
		'chat.startChatting': 'Start chatting to see your conversations here',
		'chat.quickActions': 'Quick actions',
		'chat.attachments.photos': 'Add photos or videos',
		'chat.attachments.3d': 'Add 3D objects',
		'chat.attachments.files': 'Add files (docs, PDF...)',
		'chat.attachments.description.photos': 'Upload images or video files',
		'chat.attachments.description.3d': 'Import 3D models and objects',
		'chat.attachments.description.files': 'Upload documents and files',
		'chat.tools': 'Tools',
		'chat.history': 'History',
		'chat.voiceInput': 'Voice input',
		'chat.sendMessage': 'Send message',
		'chat.addAttachments': 'Add attachments',
		// Additional chat UI translations
		'chat.tools.search': 'Web Search',
		'chat.tools.calculator': 'Calculator',
		'chat.tools.translate': 'Translate',
		'chat.tools.imageGen': 'Generate Image',
		'chat.tools.description.search': 'Search the web for information',
		'chat.tools.description.calculator': 'Perform calculations',
		'chat.tools.description.translate': 'Translate text to other languages',
		'chat.tools.description.imageGen': 'Create images from text',
		'chat.history.sample1': 'Previous conversation about React',
		'chat.history.sample2': 'Image generation request',
		'chat.history.sample3': 'Code review discussion',
		'chat.history.preview1': 'How do I optimize React performance...',
		'chat.history.preview2': 'Generate an image of a futuristic city...',
		'chat.history.preview3': 'Can you review this TypeScript code...',
		'chat.time.hoursAgo': '{{count}} hours ago',
		'chat.time.dayAgo': '1 day ago',
		'chat.time.daysAgo': '{{count}} days ago',
		'chat.back': 'Back',
		'chat.appName': 'Aquatiq Chat',
		'chat.avatar.user': 'User avatar',
		'error.network': 'Network error occurred',
		'error.generic': 'An error occurred',
		'error.copy': 'Failed to copy text',
		'common.retry': 'Retry',
		'common.updated': 'Updated',
		'common.refresh': 'Refresh',
		'common.dismiss': 'Dismiss',
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
		'traffic.accurateLocation': 'Get accurate location',
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
		'chat.placeholder.variant6': 'Fortell meg hva du trenger...',
		'chat.placeholder.variant7': 'Start en samtale...',
		'chat.placeholder.variant8': 'Hva kan jeg gjøre for deg?',
		'chat.placeholder.variant9': 'Hvordan kan jeg bistå?',
		'chat.placeholder.variant10': 'Del tankene dine...',
		'chat.button.send': 'Send',
		'chat.button.newChat': 'Ny Chat',
		'chat.status.online': 'Pålogget',
		'chat.status.typing': 'skriver...',
		'chat.welcome.title': 'Velkommen til Aquatiq Chat',
		'chat.welcome.description': 'Din intelligente AI-assistent',
		'chat.empty.title': 'Start en samtale',
		'chat.empty.description': 'Spør meg om hva som helst eller beskriv hva du vil lage.',
		'chat.action.scrollBottom': 'Rull til bunnen',
		'chat.action.copy': 'Kopier',
		'chat.action.regenerate': 'Generer på nytt',
		'chat.action.share': 'Del',
		'chat.action.like': 'Bra svar',
		'chat.action.dislike': 'Dårlig svar',
		'chat.action.copied': 'Kopiert!',
		'chat.thinking': 'tenker',
		'chat.justNow': 'akkurat nå',
		'chat.timeAgo.minutes': '{{count}}m siden',
		'chat.timeAgo.hours': '{{count}}t siden',
		'chat.timeAgo.days': '{{count}}d siden',
		'chat.models.aquatiqGpt': 'Aquatiq Gpt',
		'chat.models.chat5': 'Chat 5',
		'chat.models.chatBilder': 'Chat Bilder',
		'chat.models.selectModel': 'Velg modell',
		'chat.models.description.aquatiqGpt': 'Balansert ytelse',
		'chat.models.description.chat5': 'Forbedret kreativitet',
		'chat.models.description.chatBilder': 'Generere bilder',
		'chat.recentChats': 'Nylige chatter',
		'chat.noRecentChats': 'Ingen nylige chatter',
		'chat.startChatting': 'Start å chatte for å se samtalene dine her',
		'chat.quickActions': 'Hurtighandlinger',
		'chat.attachments.photos': 'Legg til bilder eller videoer',
		'chat.attachments.3d': 'Legg til 3D-objekter',
		'chat.attachments.files': 'Legg til filer (docs, PDF...)',
		'chat.attachments.description.photos': 'Last opp bilder eller videofiler',
		'chat.attachments.description.3d': 'Importer 3D-modeller og objekter',
		'chat.attachments.description.files': 'Last opp dokumenter og filer',
		'chat.tools': 'Verktøy',
		'chat.history': 'Historikk',
		'chat.voiceInput': 'Taleinput',
		'chat.sendMessage': 'Send melding',
		'chat.addAttachments': 'Legg til vedlegg',
		// Additional chat UI translations
		'chat.tools.search': 'Nettsøk',
		'chat.tools.calculator': 'Kalkulator',
		'chat.tools.translate': 'Oversett',
		'chat.tools.imageGen': 'Generer bilde',
		'chat.tools.description.search': 'Søk på nettet etter informasjon',
		'chat.tools.description.calculator': 'Utfør beregninger',
		'chat.tools.description.translate': 'Oversett tekst til andre språk',
		'chat.tools.description.imageGen': 'Lag bilder fra tekst',
		'chat.history.sample1': 'Tidligere samtale om React',
		'chat.history.sample2': 'Bildegenerering forespørsel',
		'chat.history.sample3': 'Kodegjennomgang diskusjon',
		'chat.history.preview1': 'Hvordan optimaliserer jeg React ytelse...',
		'chat.history.preview2': 'Generer et bilde av en futuristisk by...',
		'chat.history.preview3': 'Kan du gjennomgå denne TypeScript koden...',
		'chat.time.hoursAgo': '{{count}} timer siden',
		'chat.time.dayAgo': '1 dag siden',
		'chat.time.daysAgo': '{{count}} dager siden',
		'chat.back': 'Tilbake',
		'chat.appName': 'Aquatiq Chat',
		'chat.avatar.user': 'Bruker avatar',
		'error.network': 'Nettverksfeil oppstod',
		'error.generic': 'En feil oppstod',
		'error.copy': 'Kunne ikke kopiere tekst',
		'common.retry': 'Prøv igjen',
		'common.updated': 'Oppdatert',
		'common.refresh': 'Oppdater',
		'common.dismiss': 'Lukk',
		'dashboard.greetings.morning': 'God morgen',
		'dashboard.greetings.afternoon': 'God ettermiddag',
		'dashboard.greetings.evening': 'God kveld',
		'dashboard.subtitle': 'Din produktivitets- og innsiktshub',
		'dashboard.chatPlaceholder': 'Spør Aquatiq om noe eller beskriv en oppgave...',
		'dashboard.welcomeTitle': 'Velkommen til Aquatiq',
		'dashboard.welcomeText': 'Start din første AI-samtale for å utforske innsikt og effektivisere arbeidet.',
		'dashboard.startFirstChat': 'Start første chat',
		'dashboard.exploreFeatures': 'Utforsk kraftige funksjoner',
		'dashboard.features.ai': 'AI-assistent',
		'dashboard.features.fast': 'Raske svar',
		'dashboard.features.search': 'Smart søk',
		'dashboard.features.creative': 'Kreative ideer',
		'weather.title': 'Vær',
		'weather.location': 'Oslo, Norge',
		'weather.useMyLocation': 'Bruk min posisjon',
		'weather.unavailable': 'Vær utilgjengelig',
		'weather.wind': 'Vind',
		'weather.humidity': 'Luftfuktighet',
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
	},
};

interface I18nContextValue {
	locale: Locale;
	t: (key: string, vars?: Record<string, string | number>) => string;
	changeLocale: (l: Locale) => void;
	getPlaceholders: () => string[];
	interpolate: (template: string, vars?: Record<string, string | number>) => string;
}

const I18nAppContext = createContext<I18nContextValue | undefined>(undefined);

const AppI18nProvider: React.FC<{ children: React.ReactNode; initialLocale?: Locale }> = ({ children, initialLocale = 'en' }) => {
	const globalLang = useGlobalLanguage();
	const [locale, setLocale] = useState<Locale>(() => globalLang.getChatLocale());

	// Listen for global language changes
	useEffect(() => {
		if (typeof window === 'undefined') return;

		const handleGlobalLanguageChange = (event: CustomEvent) => {
			const { chatLocale } = event.detail;
			setLocale((currentLocale) => currentLocale === chatLocale ? currentLocale : chatLocale);
		};

		window.addEventListener('global-language-changed', handleGlobalLanguageChange as EventListener);
		
		return () => {
			window.removeEventListener('global-language-changed', handleGlobalLanguageChange as EventListener);
		};
	}, []);

	const changeLocale = useCallback((newLocale: Locale) => {
		if (!baseTranslations[newLocale]) return;
		
		// Update global language instead of local state
		const authLocale = newLocale === 'no' ? 'nb' : 'en';
		globalLang.changeLocale(authLocale);
	}, [globalLang]);

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
		t('chat.placeholder.variant6'),
		t('chat.placeholder.variant7'),
		t('chat.placeholder.variant8'),
		t('chat.placeholder.variant9'),
		t('chat.placeholder.variant10'),
	]), [t]);

	const value = useMemo<I18nContextValue>(() => ({ locale, t, changeLocale, getPlaceholders, interpolate }), [locale, t, changeLocale, getPlaceholders, interpolate]);

	return <I18nAppContext.Provider value={value}>{children}</I18nAppContext.Provider>;
};

export const useI18n = (_initialLocale?: Locale) => {
	const ctx = useContext(I18nAppContext);
	const globalLang = useGlobalLanguage();
	const [fallbackLocale, setFallbackLocale] = useState<Locale>(() => 
		_initialLocale || globalLang.getChatLocale()
	);

	useEffect(() => {
		if (typeof window === 'undefined') return;

		const handleGlobalLanguageChange = (event: CustomEvent) => {
			const { chatLocale } = event.detail;
			setFallbackLocale((currentLocale) => currentLocale === chatLocale ? currentLocale : chatLocale);
		};

		window.addEventListener('global-language-changed', handleGlobalLanguageChange as EventListener);

		return () => {
			window.removeEventListener('global-language-changed', handleGlobalLanguageChange as EventListener);
		};
	}, []);
	
	const fbInterpolate = useCallback((template: string, vars?: Record<string, string | number>) => {
		if (!vars) return template;
		return Object.entries(vars).reduce((acc, [k, v]) => acc.replace(new RegExp(`{{${k}}}`, 'g'), String(v)), template);
	}, []);
	
	const fbT = useCallback((key: string, vars?: Record<string, string | number>) => {
		const dict = baseTranslations[fallbackLocale] || {};
		const template = dict[key] ?? key;
		return fbInterpolate(template, vars);
	}, [fallbackLocale, fbInterpolate]);
	
	const fbChange = useCallback((l: Locale) => {
		// Update global language instead of local state
		const authLocale = l === 'no' ? 'nb' : 'en';
		globalLang.changeLocale(authLocale);
	}, [globalLang]);
	
	const fbPlaceholders = useCallback(() => ([
		fbT('chat.placeholder.default'),
		fbT('chat.placeholder.variant1'),
		fbT('chat.placeholder.variant2'),
		fbT('chat.placeholder.variant3'),
		fbT('chat.placeholder.variant4'),
		fbT('chat.placeholder.variant5'),
		fbT('chat.placeholder.variant6'),
		fbT('chat.placeholder.variant7'),
		fbT('chat.placeholder.variant8'),
		fbT('chat.placeholder.variant9'),
		fbT('chat.placeholder.variant10'),
	]), [fbT]);

	if (ctx) return ctx;
	return { locale: fallbackLocale, t: fbT, changeLocale: fbChange, getPlaceholders: fbPlaceholders, interpolate: fbInterpolate } as I18nContextValue;
};

const translations = baseTranslations;
