import { header } from './header';
import { footer } from './footer';
import { EMAIL_CONSTANTS } from './email-constants';

export interface EmailVerificationTemplateProps {
  userEmail: string;
  userName?: string;
  verificationUrl: string;
  companyName: string;
  supportEmail: string;
}

export function generateEmailVerificationTemplate({
  userEmail,
  userName,
  verificationUrl,
}: EmailVerificationTemplateProps): {
  subject: string;
  html: string;
  text: string;
} {
  const displayName = userName
    ? userName.split(' ')[0]
    : userEmail.split('@')[0];

  const subject = `Bekreft e-postadressen din for ${EMAIL_CONSTANTS.COMPANY.NAME}`;

  const html = `
<!DOCTYPE html>
<html lang="no">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>Bekreft E-post - ${EMAIL_CONSTANTS.COMPANY.NAME}</title>
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
            color: #1e293b;
            margin: 0 0 32px;
            line-height: 1.2;
        }
        .description {
            font-size: 16px;
            color: #64748b;
            margin: 0 0 32px;
            line-height: 1.5;
        }
        .button-container {
            margin: 32px 0;
        }
        .verify-button {
            background: linear-gradient(135deg, ${EMAIL_CONSTANTS.BRANDING.ACCENT_COLORS.SUCCESS} 0%, #059669 100%);
            color: white;
            padding: 16px 32px;
            text-decoration: none;
            border-radius: 12px;
            font-weight: 600;
            font-size: 16px;
            display: inline-block;
            transition: transform 0.2s ease;
            box-shadow: 0 4px 12px rgba(16, 185, 129, 0.3);
        }
        .verify-button:hover {
            transform: translateY(-1px);
        }
        .security-notice {
            background: #f0f9ff;
            border: 1px solid #0ea5e9;
            border-radius: 12px;
            padding: 16px;
            margin: 24px 0;
            display: flex;
            align-items: flex-start;
            gap: 12px;
        }
        .info-icon {
            width: 20px;
            height: 20px;
            background: ${EMAIL_CONSTANTS.BRANDING.ACCENT_COLORS.INFO};
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
            color: #0c4a6e;
            line-height: 1.4;
            margin: 0;
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
            <div class="success-indicator">✉️</div>
            
            <div class="email-content">
                <p class="greeting">Hei ${displayName},</p>
                <h1 class="title">Bekreft e-postadressen din</h1>
                
                <p class="description">Velkommen til ${EMAIL_CONSTANTS.COMPANY.NAME}! For å fullføre registreringen og sikre kontoen din, må du bekrefte e-postadressen din ved å klikke på knappen nedenfor.</p>
                
                <div class="button-container">
                    <a href="${verificationUrl}" class="verify-button">Bekreft e-postadresse</a>
                </div>
                
                <div class="security-notice">
                    <div class="info-icon">i</div>
                    <p class="security-text">Denne verifiseringslenken utløper om ${EMAIL_CONSTANTS.SECURITY.VERIFICATION_EXPIRY_HOURS} timer av sikkerhetshensyn. Hvis du ikke opprettet en konto hos ${EMAIL_CONSTANTS.COMPANY.NAME}, kan du trygt ignorere denne e-posten.</p>
                </div>
            </div>
        </div>
        
        ${footer()}
    </div>
</body>
</html>
  `;

  const text = `
Hei ${displayName}!

Velkommen til ${EMAIL_CONSTANTS.COMPANY.NAME}! For å fullføre registreringen og sikre kontoen din, må du bekrefte e-postadressen din.

Verifiseringslenke: ${verificationUrl}

Denne verifiseringslenken utløper om ${EMAIL_CONSTANTS.SECURITY.VERIFICATION_EXPIRY_HOURS} timer av sikkerhetshensyn. Hvis du ikke opprettet en konto hos ${EMAIL_CONSTANTS.COMPANY.NAME}, kan du trygt ignorere denne e-posten.

---
${EMAIL_CONSTANTS.COMPANY.NAME}
Sikre autentiseringstjenester

Denne e-posten ble sendt til ${userEmail}.
  `;

  return { subject, html, text };
}
