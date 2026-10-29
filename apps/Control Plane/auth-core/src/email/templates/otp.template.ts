import { header } from './header';
import { footer } from './footer';
import { EMAIL_CONSTANTS } from './email-constants';

export interface OtpTemplateProps {
  userEmail: string;
  userName?: string;
  otp: string;
  type: 'sign-in' | 'email-verification' | 'forget-password';
  companyName: string;
  supportEmail: string;
  expiresInMinutes: number;
}

export interface EmailOtpParams {
  email: string;
  code: string;
  expiresInSeconds?: number;
}

export const emailOtpEmail = (p: EmailOtpParams) => `
  <!DOCTYPE html>
  <html lang="no">
  <head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>Engangskode - ${EMAIL_CONSTANTS.COMPANY.NAME}</title>
    <style>
      body {
        margin: 0;
        padding: 20px;
        background: #f8fafc;
        font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif;
        min-height: 100vh;
        display: flex;
        align-items: center;
        justify-content: center;
      }
      .email-preview {
        width: 100%;
        max-width: 480px;
        margin: 20px;
      }
      .email-card {
        background: white;
        border-radius: 24px;
        padding: 0;
        box-shadow: 0 20px 40px rgba(0, 0, 0, 0.1);
        overflow: hidden;
        position: relative;
        margin: 24px 0;
      }
      .email-content {
        padding: 32px;
        text-align: center;
      }
      .greeting {
        font-size: 16px;
        color: #64748b;
        margin: 0 0 8px;
        font-weight: 400;
      }
      .title {
        font-size: 24px;
        font-weight: 700;
        color: ${EMAIL_CONSTANTS.BRANDING.PRIMARY_COLOR};
        margin: 0 0 32px;
        line-height: 1.2;
      }
      .code-container {
        background: #f8fafc;
        border-radius: 16px;
        padding: 32px;
        margin: 32px 0;
        border: 1px solid #e2e8f0;
      }
      .code {
        font-size: 36px;
        font-weight: 800;
        font-family: 'SF Mono', Monaco, monospace;
        letter-spacing: 8px;
        color: ${EMAIL_CONSTANTS.BRANDING.PRIMARY_COLOR};
        margin: 0;
      }
      .security-notice {
        background: #fef3c7;
        border: 1px solid #fbbf24;
        border-radius: 12px;
        padding: 16px;
        margin: 24px 0;
        display: flex;
        align-items: flex-start;
        gap: 12px;
      }
      .warning-icon {
        width: 20px;
        height: 20px;
        background: ${EMAIL_CONSTANTS.BRANDING.ACCENT_COLORS.WARNING};
        border-radius: 50%;
        flex-shrink: 0;
        display: flex;
        align-items: center;
        justify-content: center;
        color: white;
        font-size: 12px;
        font-weight: 700;
        margin-top: 2px;
      }
      .security-text {
        font-size: 14px;
        color: #92400e;
        line-height: 1.4;
        margin: 0;
      }
      .expiry-info {
        font-size: 14px;
        color: #64748b;
        margin: 24px 0 0;
        line-height: 1.4;
      }
      .success-indicator {
        position: absolute;
        top: 20px;
        right: 20px;
        width: 32px;
        height: 32px;
        background: ${EMAIL_CONSTANTS.BRANDING.ACCENT_COLORS.SUCCESS};
        border-radius: 50%;
        display: flex;
        align-items: center;
        justify-content: center;
        color: white;
        font-size: 16px;
      }
    </style>
  </head>
  <body>
    <div class="email-preview">
      ${header()}
      
      <div class="email-card">
        <div class="success-indicator">✓</div>
        
        <div class="email-content">
          <p class="greeting">Hei ${p.email},</p>
          <h1 class="title">Din engangskode</h1>
          
          <div class="code-container">
            <div class="code">${p.code}</div>
          </div>
          
          <div class="security-notice">
            <div class="warning-icon">!</div>
            <p class="security-text">Ikke del denne koden med noen. ${EMAIL_CONSTANTS.COMPANY.NAME} vil aldri spørre om koden din.</p>
          </div>
          
          <p class="expiry-info">Koden utløper${p.expiresInSeconds ? ` om ${Math.ceil(p.expiresInSeconds / 60)} minutter` : ` om ${EMAIL_CONSTANTS.SECURITY.OTP_DEFAULT_EXPIRY_MINUTES} minutter`}.<br>Hvis du ikke ba om denne koden, kan du trygt ignorere denne e-posten.</p>
        </div>
      </div>
      
      ${footer()}
    </div>
  </body>
  </html>
`;

export function generateOtpTemplate({
  userEmail,
  userName,
  otp,
  type,
  expiresInMinutes = EMAIL_CONSTANTS.SECURITY.OTP_DEFAULT_EXPIRY_MINUTES,
}: OtpTemplateProps): {
  subject: string;
  html: string;
  text: string;
} {
  const displayName = userName
    ? userName.split(' ')[0]
    : userEmail.split('@')[0];

  const typeConfig = {
    'sign-in': {
      subject: `Din ${EMAIL_CONSTANTS.COMPANY.NAME} påloggingskode`,
      title: 'Påloggingskode',
      message: 'Bruk denne koden for å fullføre påloggingen:',
      icon: '🔑',
      color: EMAIL_CONSTANTS.BRANDING.ACCENT_COLORS.INFO,
    },
    'email-verification': {
      subject: `Verifiser din ${EMAIL_CONSTANTS.COMPANY.NAME} e-post`,
      title: 'E-postverifiseringskode',
      message: 'Bruk denne koden for å verifisere e-postadressen din:',
      icon: '✉️',
      color: EMAIL_CONSTANTS.BRANDING.ACCENT_COLORS.SUCCESS,
    },
    'forget-password': {
      subject: `Din ${EMAIL_CONSTANTS.COMPANY.NAME} kode for tilbakestilling av passord`,
      title: 'Kode for tilbakestilling av passord',
      message: 'Bruk denne koden for å tilbakestille passordet ditt:',
      icon: '🔐',
      color: EMAIL_CONSTANTS.BRANDING.ACCENT_COLORS.DANGER,
    },
  };

  const config = typeConfig[type];
  const subject = config.subject;

  // Use the new template design
  const html = emailOtpEmail({
    email: userEmail,
    code: otp,
    expiresInSeconds: expiresInMinutes * 60,
  });

  const text = `
Hei ${displayName}!

${config.message}

Din ${config.title.toLowerCase()}: ${otp}

⏰ Denne koden utløper om ${expiresInMinutes} minutter.

🛡️ Sikkerhetsnotis: Denne koden er kun til engangsbruk. Hvis du ikke ba om denne koden, kan du ignorere denne e-posten og kontakte oss hvis du har spørsmål.

---
${EMAIL_CONSTANTS.COMPANY.NAME}
Sikre autentiseringstjenester

Denne e-posten ble sendt til ${userEmail}.
  `;

  return { subject, html, text };
}
