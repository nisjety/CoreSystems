import { Shield, Info, ExternalLink } from 'lucide-react';
import { useLanguageSwitch } from '../lib/i18n/hooks';

interface GDPRTextProps {
  variant?: 'registration' | 'login' | 'data-processing' | 'minimal' | 'norwegian-simple';
  showIcon?: boolean;
  className?: string;
  linkClassName?: string;
  language?: 'no' | 'en';
}

export function GDPRText({ 
  variant = 'norwegian-simple',
  showIcon = true,
  className = '',
  linkClassName = 'text-primary hover:text-primary/80 underline',
  language
}: GDPRTextProps) {
  // Use i18n hook for language detection
  const { isNorwegian } = useLanguageSwitch();
  const currentLanguage = language || (isNorwegian ? 'no' : 'en');

  const baseClassName = `text-xs text-muted-foreground ${className}`;

  // Localized texts
  const texts = {
    no: {
      deleteData: 'Vi lagrer kun nødvendig innloggingsdata lokalt på din enhet, med automatisk utløp. Du kan når som helst slette dette',
      deleteCookie: 'Slett Cookie',
      protectedBy: 'Beskyttet av vår',
      privacyPolicy: 'Personvernpolicy',
      byContinuing: 'Ved å fortsette aksepterer du våre',
      terms: 'brukervilkår',
      and: 'og',
      privacyRules: 'personvernregler',
      dataProcessingNotice: 'Databehandlingsnotis',
      whatWeCollect: 'Hva vi samler inn:',
      whatWeCollectDesc: 'E-post, navn, autentiseringsdata og bruksanalyse.',
      whyWeCollect: 'Hvorfor vi samler det:',
      whyWeCollectDesc: 'For å tilby sikker autentisering, forbedre tjenesten og overholde juridiske krav.',
      yourRights: 'Dine rettigheter:',
      yourRightsDesc: 'Du kan når som helst få tilgang til, rette, slette eller portere dine data.',
      fullPrivacyPolicy: 'Full personvernpolicy',
      gdprRights: 'Dine GDPR-rettigheter',
      requestData: 'Forespør dine data',
      gdprCompliant: 'GDPR-kompatibel',
      byCreating: 'Ved å opprette en konto godtar du våre',
      termsOfService: 'Tjenestevilkår',
      confirms: 'og bekrefter at vi vil behandle dine personopplysninger i samsvar med vår',
      managePreferences: 'Du kan administrere dine datapreferanser og utøve dine GDPR-rettigheter når som helst i dine',
      privacySettings: 'personverninnstillinger'
    },
    en: {
      deleteData: 'We only store necessary login data locally on your device, with automatic expiration. You can delete this at any time',
      deleteCookie: 'Delete Cookie',
      protectedBy: 'Protected by our',
      privacyPolicy: 'Privacy Policy',
      byContinuing: 'By continuing you accept our',
      terms: 'terms of service',
      and: 'and',
      privacyRules: 'privacy policy',
      dataProcessingNotice: 'Data Processing Notice',
      whatWeCollect: 'What we collect:',
      whatWeCollectDesc: 'Email, name, authentication data, and usage analytics.',
      whyWeCollect: 'Why we collect it:',
      whyWeCollectDesc: 'To provide secure authentication, improve the service, and comply with legal requirements.',
      yourRights: 'Your rights:',
      yourRightsDesc: 'You can access, correct, delete, or port your data at any time.',
      fullPrivacyPolicy: 'Full privacy policy',
      gdprRights: 'Your GDPR rights',
      requestData: 'Request your data',
      gdprCompliant: 'GDPR Compliant',
      byCreating: 'By creating an account you agree to our',
      termsOfService: 'Terms of Service',
      confirms: 'and confirm that we will process your personal information in accordance with our',
      managePreferences: 'You can manage your data preferences and exercise your GDPR rights at any time in your',
      privacySettings: 'privacy settings'
    }
  };

  const t = texts[currentLanguage];

  if (variant === 'norwegian-simple') {
    return (
      <div className={baseClassName}>
        <p className="leading-relaxed">
          {t.deleteData}{' '}
          <button 
            type="button"
            onClick={() => {
              // Cookie deletion logic matching reference script
              document.cookie.split(";").forEach(function(c) { 
                document.cookie = c.replace(/^ +/, "").replace(/=.*/, "=;expires=" + new Date().toUTCString() + ";path=/"); 
              });
              localStorage.clear();
              sessionStorage.clear();
            }}
            className={`${linkClassName} focus:outline-none`}
          >
            {t.deleteCookie}
          </button>.
        </p>
      </div>
    );
  }

  if (variant === 'minimal') {
    return (
      <p className={baseClassName}>
        {showIcon && <Shield className="w-3 h-3 inline mr-1" />}
        {t.protectedBy}{' '}
        <a href="/personvern" className={linkClassName}>
          {t.privacyPolicy}
        </a>
      </p>
    );
  }

  if (variant === 'login') {
    return (
      <div className={baseClassName}>
        <p className="leading-relaxed mb-3">
          {t.byContinuing}{' '}
          <button type="button" className={`${linkClassName} focus:outline-none`}>
            {t.terms}
          </button>{' '}
          {t.and}{' '}
          <button type="button" className={`${linkClassName} focus:outline-none`}>
            {t.privacyRules}
          </button>.
        </p>
      </div>
    );
  }

  if (variant === 'data-processing') {
    return (
      <div className={`${baseClassName} space-y-3 p-4 bg-blue-50 border border-blue-200 rounded-lg`}>
        {showIcon && (
          <div className="flex items-center gap-2">
            <Shield className="w-5 h-5 text-blue-600" />
            <h4 className="font-medium text-blue-900">{t.dataProcessingNotice}</h4>
          </div>
        )}
        <div className="space-y-2 text-blue-800">
          <p>
            <strong>{t.whatWeCollect}</strong> {t.whatWeCollectDesc}
          </p>
          <p>
            <strong>{t.whyWeCollect}</strong> {t.whyWeCollectDesc}
          </p>
          <p>
            <strong>{t.yourRights}</strong> {t.yourRightsDesc}
          </p>
        </div>
        <div className="flex flex-wrap gap-4 text-blue-700">
          <a href="/personvern" className={linkClassName}>
            {t.fullPrivacyPolicy}
            <ExternalLink className="w-3 h-3 inline ml-1" />
          </a>
          <a href="/gdpr-rettigheter" className={linkClassName}>
            {t.gdprRights}
            <ExternalLink className="w-3 h-3 inline ml-1" />
          </a>
          <a href="/dataforespørsel" className={linkClassName}>
            {t.requestData}
            <ExternalLink className="w-3 h-3 inline ml-1" />
          </a>
        </div>
      </div>
    );
  }

  // Default 'registration' variant
  return (
    <div className={baseClassName}>
      {showIcon && (
        <div className="flex items-start gap-2 mb-2">
          <Shield className="w-4 h-4 text-green-500 mt-0.5 flex-shrink-0" />
          <div>
            <p className="font-medium text-foreground mb-1">{t.gdprCompliant}</p>
          </div>
        </div>
      )}
      <p className="leading-relaxed">
        {t.byCreating}{' '}
        <a href="/vilkår" className={linkClassName}>
          {t.termsOfService}
          <ExternalLink className="w-3 h-3 inline ml-1" />
        </a>{' '}
        {t.confirms}{' '}
        <a href="/personvern" className={linkClassName}>
          {t.privacyPolicy}
          <ExternalLink className="w-3 h-3 inline ml-1" />
        </a>
        . {t.managePreferences}{' '}
        <a href="/innstillinger/personvern" className={linkClassName}>
          {t.privacySettings}
        </a>
        .
      </p>
    </div>
  );
}

// Simplified consent checkbox component
interface GDPRConsentProps {
  checked: boolean;
  onChange: (checked: boolean) => void;
  required?: boolean;
  className?: string;
  language?: 'no' | 'en';
}

export function GDPRConsent({ 
  checked, 
  onChange, 
  required = true,
  className = '',
  language
}: GDPRConsentProps) {
  // Use i18n hook for language detection
  const { isNorwegian } = useLanguageSwitch();
  const currentLanguage = language || (isNorwegian ? 'no' : 'en');

  const consentTexts = {
    no: {
      agree: 'Jeg godtar',
      terms: 'Tjenestevilkårene',
      and: 'og',
      privacy: 'Personvernpolicyen'
    },
    en: {
      agree: 'I agree to the',
      terms: 'Terms of Service',
      and: 'and',
      privacy: 'Privacy Policy'
    }
  };

  const ct = consentTexts[currentLanguage];

  return (
    <div className={`flex items-start gap-3 ${className}`}>
      <input
        type="checkbox"
        id="gdpr-consent"
        checked={checked}
        onChange={(e) => onChange(e.target.checked)}
        required={required}
        className="mt-0.5 h-4 w-4 text-primary border-border rounded focus:ring-primary focus:ring-2"
      />
      <label htmlFor="gdpr-consent" className="text-sm text-foreground cursor-pointer">
        {ct.agree}{' '}
        <a href="/vilkår" className="text-primary hover:text-primary/80 underline">
          {ct.terms}
        </a>{' '}
        {ct.and}{' '}
        <a href="/personvern" className="text-primary hover:text-primary/80 underline">
          {ct.privacy}
        </a>
        {required && <span className="text-destructive ml-1">*</span>}
      </label>
    </div>
  );
}

// Cookie consent component
interface CookieConsentProps {
  onAccept: () => void;
  onDecline?: () => void;
  showDecline?: boolean;
  className?: string;
  language?: 'no' | 'en';
}

export function CookieConsent({ 
  onAccept, 
  onDecline,
  showDecline = true,
  className = '',
  language
}: CookieConsentProps) {
  // Use i18n hook for language detection
  const { isNorwegian } = useLanguageSwitch();
  const currentLanguage = language || (isNorwegian ? 'no' : 'en');

  const cookieTexts = {
    no: {
      title: 'Informasjonskapsler',
      description: 'Vi bruker nødvendige informasjonskapsler for autentisering og valgfrie for analyse. Se vår',
      cookiePolicy: 'Informasjonskapselretningslinjer',
      forDetails: 'for detaljer.',
      acceptAll: 'Godta alle',
      onlyNecessary: 'Kun nødvendige'
    },
    en: {
      title: 'Cookies',
      description: 'We use necessary cookies for authentication and optional ones for analytics. See our',
      cookiePolicy: 'Cookie Policy',
      forDetails: 'for details.',
      acceptAll: 'Accept All',
      onlyNecessary: 'Only Necessary'
    }
  };

  const ckt = cookieTexts[currentLanguage];

  return (
    <div className={`fixed bottom-4 left-4 right-4 z-50 max-w-md mx-auto ${className}`}>
      <div className="bg-card border border-border rounded-lg shadow-lg p-4">
        <div className="flex items-start gap-3">
          <Info className="w-5 h-5 text-primary mt-0.5 flex-shrink-0" />
          <div className="flex-1">
            <h3 className="font-medium text-foreground mb-2">{ckt.title}</h3>
            <p className="text-sm text-muted-foreground mb-3">
              {ckt.description}{' '}
              <a href="/personvern#informasjonskapsler" className="text-primary hover:text-primary/80 underline">
                {ckt.cookiePolicy}
              </a>{' '}
              {ckt.forDetails}
            </p>
            <div className="flex gap-2">
              <button
                onClick={onAccept}
                className="px-3 py-1.5 bg-primary text-primary-foreground text-sm font-medium rounded hover:bg-primary/90 focus:outline-none focus:ring-2 focus:ring-primary focus:ring-offset-1"
              >
                {ckt.acceptAll}
              </button>
              {showDecline && onDecline && (
                <button
                  onClick={onDecline}
                  className="px-3 py-1.5 border border-border text-foreground text-sm font-medium rounded hover:bg-muted focus:outline-none focus:ring-2 focus:ring-ring focus:ring-offset-1"
                >
                  {ckt.onlyNecessary}
                </button>
              )}
            </div>
          </div>
        </div>
      </div>
    </div>
  );
}

// Data retention notice
interface DataRetentionNoticeProps {
  className?: string;
  language?: 'no' | 'en';
}

export function DataRetentionNotice({ 
  className = '',
  language
}: DataRetentionNoticeProps) {
  // Use i18n hook for language detection
  const { isNorwegian } = useLanguageSwitch();
  const currentLanguage = language || (isNorwegian ? 'no' : 'en');

  const retentionTexts = {
    no: {
      title: 'Datalagring',
      description: 'Kontodata lagres så lenge kontoen er aktiv. Autentiseringslogger oppbevares i 90 dager av sikkerhetshensyn. Du kan be om sletting av data når som helst gjennom dine',
      privacySettings: 'personverninnstillinger'
    },
    en: {
      title: 'Data Retention',
      description: 'Account data is stored as long as the account is active. Authentication logs are kept for 90 days for security reasons. You can request data deletion at any time through your',
      privacySettings: 'privacy settings'
    }
  };

  const rt = retentionTexts[currentLanguage];

  return (
    <div className={`p-3 bg-muted border border-border rounded text-xs text-muted-foreground ${className}`}>
      <div className="flex items-start gap-2">
        <Shield className="w-4 h-4 text-muted-foreground mt-0.5 flex-shrink-0" />
        <div>
          <p className="font-medium mb-1">{rt.title}</p>
          <p>
            {rt.description}{' '}
            <a href="/innstillinger/personvern" className="text-primary hover:text-primary/80 underline">
              {rt.privacySettings}
            </a>
            .
          </p>
        </div>
      </div>
    </div>
  );
}