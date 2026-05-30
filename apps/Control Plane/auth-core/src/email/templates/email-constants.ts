export const EMAIL_CONSTANTS = {
  COMPANY: {
    NAME: 'ID-Knuten',
    SUPPORT_EMAIL: 'support@idknuten.no',
    PHONE: '+47 62 52 00 00',
    ADDRESS: {
      STREET: 'Storgata 15',
      POSTAL_CODE: '2317',
      CITY: 'Hamar',
      REGION: 'Innlandet',
      COUNTRY: 'Norge',
    },
  },
  BRANDING: {
    LOGO_INITIALS: 'IK',
    PRIMARY_COLOR: '#1e293b',
    ACCENT_COLORS: {
      SUCCESS: '#10b981',
      WARNING: '#f59e0b',
      DANGER: '#ef4444',
      INFO: '#3b82f6',
      PURPLE: '#8b5cf6',
    },
  },
  SECURITY: {
    VERIFICATION_EXPIRY_HOURS: 24,
    PASSWORD_RESET_EXPIRY_HOURS: 1,
    OTP_DEFAULT_EXPIRY_MINUTES: 5,
  },
} as const;
