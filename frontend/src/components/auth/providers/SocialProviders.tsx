import React from 'react';
import { useAuthTranslation } from '../lib/i18n/hooks';

interface SocialProvidersProps {
  onProviderClick: (provider: string) => void;
  isLoading?: boolean;
  disabled?: boolean;
  className?: string;
}

// Social providers matching exact reference design
export const socialProviders = [
  {
    name: 'Microsoft',
    color: 'bg-background text-foreground border border-border hover:bg-muted',
    active: true,
    icon: (
      <svg viewBox="0 0 24 24" className="w-5 h-5" aria-hidden="true">
        <path fill="#f25022" d="M1 1h10v10H1z" />
        <path fill="#00a4ef" d="M13 1h10v10H13z" />
        <path fill="#7fba00" d="M1 13h10v10H1z" />
        <path fill="#ffb900" d="M13 13h10v10H13z" />
      </svg>
    ),
  },
  {
    name: 'Google',
    color: 'bg-background text-foreground border border-border hover:bg-muted',
    active: true,
    icon: (
      <svg viewBox="0 0 24 24" className="w-5 h-5" aria-hidden="true">
        <path fill="#4285F4" d="M22.56 12.25c0-.78-.07-1.53-.2-2.25H12v4.26h5.92c-.26 1.37-1.04 2.53-2.21 3.31v2.77h3.57c2.08-1.92 3.28-4.74 3.28-8.09z"/>
        <path fill="#34A853" d="M12 23c2.97 0 5.46-.98 7.28-2.66l-3.57-2.77c-.98.66-2.23 1.06-3.71 1.06-2.86 0-5.29-1.93-6.16-4.53H2.18v2.84C3.99 20.53 7.7 23 12 23z"/>
        <path fill="#FBBC05" d="M5.84 14.09c-.22-.66-.35-1.36-.35-2.09s.13-1.43.35-2.09V7.07H2.18C1.43 8.55 1 10.22 1 12s.43 3.45 1.18 4.93l2.85-2.22.81-.62z"/>
        <path fill="#EA4335" d="M12 5.38c1.62 0 3.06.56 4.21 1.64l3.15-3.15C17.45 2.09 14.97 1 12 1 7.7 1 3.99 3.47 2.18 7.07l3.66 2.84c.87-2.6 3.3-4.53 6.16-4.53z"/>
      </svg>
    ),
  },
  {
    name: 'Okta',
    color: 'bg-muted text-muted-foreground border border-border cursor-not-allowed',
    active: false,
    icon: (
      <svg viewBox="0 0 24 24" className="w-5 h-5" aria-hidden="true">
        <path fill="currentColor" d="M12 1.5A10.5 10.5 0 1 0 22.5 12 10.512 10.512 0 0 0 12 1.5Zm0 16.2a5.7 5.7 0 1 1 5.7-5.7A5.707 5.707 0 0 1 12 17.7Zm0-2.7a3 3 0 1 0-3-3 3.005 3.005 0 0 0 3 3Z" />
      </svg>
    ),
  },
  {
    name: 'Vipps',
    color: 'bg-muted text-muted-foreground border border-border cursor-not-allowed',
    active: false,
    icon: (
      <svg viewBox="0 0 24 24" className="w-5 h-5" aria-hidden="true">
        <path fill="currentColor" d="M6.2 8.1c.5 0 .9.4.9.9 0 3.6 2.3 6.4 5.9 6.4 2.7 0 4.1-1.4 5-3.3.2-.4.6-.7 1.1-.7.6 0 1 .4 1 .9 0 .1 0 .3-.1.4-1.1 2.6-3.3 4.7-7 4.7-4.7 0-7.8-3.6-7.8-8.4 0-.5.4-.9 1-.9zM17.8 6c.9 0 1.6.7 1.6 1.6s-.7 1.6-1.6 1.6S16.2 8.5 16.2 7.6 16.9 6 17.8 6z" />
      </svg>
    ),
  },
];

export function SocialProviders({ 
  onProviderClick, 
  isLoading = false, 
  disabled = false,
  className = ''
}: SocialProvidersProps) {
  const { authT } = useAuthTranslation();
  
  return (
    <div className={className}>
      {/* Translated divider text */}
      <div className="relative my-4">
        <div className="absolute inset-0 flex items-center">
          <div className="w-full border-t border-border" />
        </div>
        <div className="relative flex justify-center text-sm">
          <span className="px-2 bg-background text-muted-foreground">{authT.social('orWith')}</span>
        </div>
      </div>

      {/* Social provider buttons - exact grid layout from reference */}
      <div className="grid grid-cols-4 gap-2 mb-4">
        {socialProviders.map((provider) => (
          <button
            key={provider.name}
            onClick={provider.active && !disabled && !isLoading ? () => onProviderClick(provider.name.toLowerCase()) : undefined}
            disabled={isLoading || !provider.active || disabled}
            className={`relative left-6 w-10 h-10 rounded-full border border-border flex items-center justify-center transition-all ${
              provider.active ? 'hover:scale-105' : ''
            } focus:outline-none focus:ring-2 focus:ring-ring focus:ring-offset-2 disabled:opacity-50 ${provider.color}`}
            aria-label={provider.active ? authT.socialWithProvider('signInWith', provider.name) : authT.socialWithProvider('unavailable', provider.name)}
            title={provider.active ? authT.socialWithProvider('signInWith', provider.name) : authT.socialWithProvider('unavailable', provider.name)}
          >
            {provider.icon}
          </button>
        ))}
      </div>
    </div>
  );
}