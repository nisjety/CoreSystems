import { header } from './header';
import { footer } from './footer';
import { EMAIL_CONSTANTS } from './email-constants';

export interface EmailChangeTemplateProps {
  userEmail: string;
  userName?: string;
  newEmail: string;
  verificationUrl: string;
  companyName: string;
  supportEmail: string;
}

export function generateEmailChangeTemplate({
  userEmail,
  userName,
  newEmail,
  verificationUrl,
}: EmailChangeTemplateProps): {
  subject: string;
  html: string;
  text: string;
} {
  const displayName = userName
    ? userName.split(' ')[0]
    : userEmail.split('@')[0];

  const subject = `Bekreft endring av e-postadresse for ${EMAIL_CONSTANTS.COMPANY.NAME}`;

  const html = `
<!DOCTYPE html>
<html lang="no">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>Bekreft Endring av E-post - ${EMAIL_CONSTANTS.COMPANY.NAME}</title>
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
            margin: 0 0 24px;
            line-height: 1.5;
        }
        .button-container {
            margin: 32px 0;
        }
        .change-button {
            background: linear-gradient(135deg, ${EMAIL_CONSTANTS.BRANDING.ACCENT_COLORS.PURPLE} 0%, #7c3aed 100%);
            color: white;
            padding: 16px 32px;
            text-decoration: none;
            border-radius: 12px;
            font-weight: 600;
            font-size: 16px;
            display: inline-block;
            transition: transform 0.2s ease;
            box-shadow: 0 4px 12px rgba(139, 92, 246, 0.3);
        }
        .change-button:hover {
            transform: translateY(-1px);
        }
        .email-change-info {
            background: #eff6ff;
            border: 1px solid #93c5fd;
            border-radius: 12px;
            padding: 16px;
            margin: 24px 0;
            text-align: left;
        }
        .email-change-title {
            font-size: 14px;
            color: #1e40af;
            font-weight: 600;
            margin: 0 0 8px;
        }
        .email-highlight {
            font-family: monospace;
            background: #f1f5f9;
            padding: 2px 6px;
            border-radius: 4px;
            font-weight: 600;
            color: #1e293b;
        }
        .email-change-text {
            font-size: 14px;
            color: #1e40af;
            margin: 4px 0;
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
        .success-indicator {
            position: absolute;
            top: 20px;
            right: 20px;
            width: 32px;
            height: 32px;
            background: ${EMAIL_CONSTANTS.BRANDING.ACCENT_COLORS.PURPLE};
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
            <div class="success-indicator">📮</div>
            
            <div class="email-content">
                <p class="greeting">Hei ${displayName},</p>
                <h1 class="title">Bekreft endring av e-post</h1>
                
                <div class="email-change-info">
                    <p class="email-change-title">📮 Forespørsel om endring av e-postadresse:</p>
                    <p class="email-change-text">Fra: <span class="email-highlight">${userEmail}</span></p>
                    <p class="email-change-text">Til: <span class="email-highlight">${newEmail}</span></p>
                </div>
                
                <p class="description">Du ba om å endre e-postadressen for din ${EMAIL_CONSTANTS.COMPANY.NAME}-konto. For å godkjenne denne endringen, klikk på knappen nedenfor. Denne bekreftelsen sendes til din <strong>nåværende e-postadresse</strong> av sikkerhetshensyn.</p>
                
                <div class="button-container">
                    <a href="${verificationUrl}" class="change-button">Godkjenn endring av e-post</a>
                </div>
                
                <div class="security-notice">
                    <div class="warning-icon">!</div>
                    <p class="security-text">Hvis du ikke ba om denne endringen av e-postadresse, kan du ignorere denne e-posten og kontakte oss umiddelbart. E-postadressen din forblir uendret.</p>
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

📮 Forespørsel om endring av e-postadresse:
Fra: ${userEmail}
Til: ${newEmail}

Du ba om å endre e-postadressen for din ${EMAIL_CONSTANTS.COMPANY.NAME}-konto. For å godkjenne denne endringen, bruk lenken nedenfor. Denne bekreftelsen sendes til din nåværende e-postadresse av sikkerhetshensyn.

Godkjenningslenke: ${verificationUrl}

🛡️ Sikkerhetsnotis: Hvis du ikke ba om denne endringen av e-postadresse, kan du ignorere denne e-posten og kontakte oss umiddelbart. E-postadressen din forblir uendret.

---
${EMAIL_CONSTANTS.COMPANY.NAME}
Sikre autentiseringstjenester

Denne e-posten ble sendt til ${userEmail}.
  `;

  return { subject, html, text };
}
