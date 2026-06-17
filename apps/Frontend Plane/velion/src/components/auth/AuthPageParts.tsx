import React from 'react';
import { Mail } from 'lucide-react';
import type { AuthMode } from './types/auth';

// ── FormFooter ────────────────────────────────────────────────────────────

interface FormFooterProps {
  currentMode: AuthMode;
  termsPrefixSignin: string;
  termsAnd: string;
  termsUser: string;
  termsPrivacy: string;
  signupDataNotice: string;
  deleteCookie: string;
}

export function FormFooter({
  currentMode,
  termsPrefixSignin,
  termsAnd,
  termsUser,
  termsPrivacy,
  signupDataNotice,
  deleteCookie,
}: FormFooterProps) {
  return (
    <div className="mb-1.5 text-left xl:mb-2">
      {currentMode === 'signin' && (
        <p className="font-inter text-xs leading-[1.65] tracking-[0.02em] text-[#6A655F]" aria-label="terms-and-privacy">
          {termsPrefixSignin}{' '}
          <button type="button" className="text-[#3E3A35] underline hover:text-[#1C1C1C] focus:outline-none">
            {termsUser}
          </button>{' '}
          {termsAnd}{' '}
          <button type="button" className="text-[#3E3A35] underline hover:text-[#1C1C1C] focus:outline-none">
            {termsPrivacy}
          </button>
          .
        </p>
      )}
      {currentMode === 'signup' && (
        <p className="font-inter text-xs leading-[1.65] tracking-[0.02em] text-[#6A655F]" aria-label="signup-data-notice">
          {signupDataNotice}{' '}
          <button
            type="button"
            className="text-[#3E3A35] underline hover:text-[#1C1C1C] focus:outline-none"
            onClick={() => {
              document.cookie.split(';').forEach((cookiePart) => {
                document.cookie = cookiePart
                  .replace(/^ +/, '')
                  .replace(/=.*/, `=;expires=${new Date().toUTCString()};path=/`);
              });
              localStorage.clear();
              sessionStorage.clear();
            }}
          >
            {deleteCookie}
          </button>
          .
        </p>
      )}
    </div>
  );
}

// ── SupportLinks ──────────────────────────────────────────────────────────

interface SupportLinksProps {
  supportEmail: string;
  supportNeedHelp: string;
  supportContact: string;
  helpLabel: string;
  showConsentBanner: boolean;
}

export function SupportLinks({
  supportEmail,
  supportNeedHelp,
  supportContact,
  helpLabel,
  showConsentBanner,
}: SupportLinksProps) {
  return (
    <>
      <div className="mt-2 hidden text-left sm:block xl:mt-3">
        <p className="font-inter text-xs tracking-[0.02em] text-[#6A655F]">
          {supportNeedHelp}{' '}
          <a href={`mailto:${supportEmail}`} className="text-[#3E3A35] transition-colors hover:text-[#1C1C1C]">
            {supportContact}
          </a>
        </p>
      </div>

      <div className="mt-1 flex space-x-3 sm:hidden">
        {!showConsentBanner && (
          <a
            href={`mailto:${supportEmail}`}
            className="flex items-center gap-1 font-inter text-xs tracking-[0.02em] text-[#6A655F] transition-colors hover:text-[#1C1C1C]"
            aria-label={helpLabel}
          >
            <Mail className="w-3 h-3" />
            {helpLabel}
          </a>
        )}
      </div>
    </>
  );
}

// ── PageFooterLinks ───────────────────────────────────────────────────────

interface PageFooterLinksProps {
  imprint: string;
  privacy: string;
  copyright: string;
  cookieSettings: string;
  onCookieSettings: () => void;
}

export function PageFooterLinks({
  imprint,
  privacy,
  copyright,
  cookieSettings,
  onCookieSettings,
}: PageFooterLinksProps) {
  return (
    <div className="pointer-events-none absolute bottom-3 left-0 right-0 hidden text-center md:block">
      <div className="pointer-events-auto flex flex-wrap items-center justify-center gap-6 font-inter text-xs tracking-[0.02em] text-[#6A655F]">
        <button className="transition-colors hover:text-[#1C1C1C]">{imprint}</button>
        <button className="transition-colors hover:text-[#1C1C1C]">{privacy}</button>
        <button className="transition-colors hover:text-[#1C1C1C]">{copyright}</button>
        <button className="transition-colors hover:text-[#1C1C1C]" onClick={onCookieSettings}>
          {cookieSettings}
        </button>
      </div>
    </div>
  );
}
