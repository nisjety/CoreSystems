import { header } from './header';
import { footer } from './footer';
import { EMAIL_CONSTANTS } from './email-constants';

export interface PasswordResetTemplateProps {
  userEmail: string;
  userName?: string;
  resetUrl: string;
  companyName: string;
  supportEmail: string;
}

export function generatePasswordResetTemplate({
  userEmail,
  userName,
  resetUrl,
}: PasswordResetTemplateProps): {
  subject: string;
  html: string;
  text: string;
} {
  const displayName = userName
    ? userName.split(' ')[0]
    : userEmail.split('@')[0];

  const subject = `Tilbakestill passordet ditt for ${EMAIL_CONSTANTS.COMPANY.NAME}`;

  const html = `
<!DOCTYPE html>
<html lang="no">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>Tilbakestill Passord - ${EMAIL_CONSTANTS.COMPANY.NAME}</title>
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
        .reset-button {
            background: linear-gradient(135deg, ${EMAIL_CONSTANTS.BRANDING.ACCENT_COLORS.DANGER} 0%, #dc2626 100%);
            color: white;
            padding: 16px 32px;
            text-decoration: none;
            border-radius: 12px;
            font-weight: 600;
            font-size: 16px;
            display: inline-block;
            transition: transform 0.2s ease;
            box-shadow: 0 4px 12px rgba(239, 68, 68, 0.3);
        }
        .reset-button:hover {
            transform: translateY(-1px);
        }
        .warning-notice {
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
        .warning-text {
            font-size: 14px;
            color: #92400e;
            line-height: 1.4;
            margin: 0;
        }
        .security-notice {
            background: #fef2f2;
            border: 1px solid #fca5a5;
            border-radius: 12px;
            padding: 16px;
            margin: 24px 0;
            display: flex;
            align-items: flex-start;
            gap: 12px;
        }
        .security-icon {
            width: 20px;
            height: 20px;
            background: ${EMAIL_CONSTANTS.BRANDING.ACCENT_COLORS.DANGER};
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
            color: #991b1b;
            line-height: 1.4;
            margin: 0;
        }
        .success-indicator {
            position: absolute;
            top: 20px;
            right: 20px;
            width: 32px;
            height: 32px;
            background: ${EMAIL_CONSTANTS.BRANDING.ACCENT_COLORS.DANGER};
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
            <div class="success-indicator">🔒</div>
            
            <div class="email-content">
                <p class="greeting">Hei ${displayName},</p>
                <h1 class="title">Tilbakestill passord</h1>
                
                <div class="warning-notice">
                    <div class="warning-icon">!</div>
                    <p class="warning-text"><strong>Forespørsel om tilbakestilling av passord:</strong> Noen ba om å tilbakestille passordet ditt. Hvis dette ikke var deg, kan du ignorere denne e-posten.</p>
                </div>
                
                <p class="description">Du ba om å tilbakestille passordet for din ${EMAIL_CONSTANTS.COMPANY.NAME}-konto. Klikk på knappen nedenfor for å opprette et nytt passord.</p>
                
                <div class="button-container">
                    <a href="${resetUrl}" class="reset-button">Tilbakestill passord</a>
                </div>
                
                <div class="security-notice">
                    <div class="security-icon">🛡️</div>
                    <p class="security-text">Denne lenken for tilbakestilling av passord utløper om ${EMAIL_CONSTANTS.SECURITY.PASSWORD_RESET_EXPIRY_HOURS} time av sikkerhetshensyn. Hvis du ikke ba om denne tilbakestillingen, kan du trygt ignorere denne e-posten og passordet ditt forblir uendret.</p>
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

⚠️ Forespørsel om tilbakestilling av passord: Noen ba om å tilbakestille passordet ditt. Hvis dette ikke var deg, kan du ignorere denne e-posten.

Du ba om å tilbakestille passordet for din ${EMAIL_CONSTANTS.COMPANY.NAME}-konto. Bruk lenken nedenfor for å opprette et nytt passord:

Tilbakestillingslenke: ${resetUrl}

Denne lenken for tilbakestilling av passord utløper om ${EMAIL_CONSTANTS.SECURITY.PASSWORD_RESET_EXPIRY_HOURS} time av sikkerhetshensyn. Hvis du ikke ba om denne tilbakestillingen, kan du trygt ignorere denne e-posten og passordet ditt forblir uendret.

---
${EMAIL_CONSTANTS.COMPANY.NAME}
Sikre autentiseringstjenester

Denne e-posten ble sendt til ${userEmail}.
  `;

  return { subject, html, text };
}
