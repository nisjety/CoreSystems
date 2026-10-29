import { header } from './header';
import { footer } from './footer';
import { EMAIL_CONSTANTS } from './email-constants';

export interface AccountDeletionTemplateProps {
  userEmail: string;
  userName?: string;
  verificationUrl: string;
  companyName: string;
  supportEmail: string;
}

export function generateAccountDeletionTemplate({
  userEmail,
  userName,
  verificationUrl,
}: AccountDeletionTemplateProps): {
  subject: string;
  html: string;
  text: string;
} {
  const displayName = userName
    ? userName.split(' ')[0]
    : userEmail.split('@')[0];

  const subject = `Bekreft sletting av konto for ${EMAIL_CONSTANTS.COMPANY.NAME}`;

  const html = `
<!DOCTYPE html>
<html lang="no">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>Bekreft Sletting av Konto - ${EMAIL_CONSTANTS.COMPANY.NAME}</title>
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
        .delete-button {
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
        .delete-button:hover {
            transform: translateY(-1px);
        }
        .warning-notice {
            background: #fef2f2;
            border: 1px solid #fca5a5;
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
        .warning-text {
            font-size: 14px;
            color: #991b1b;
            line-height: 1.4;
            margin: 0;
        }
        .data-info {
            background: #fef3c7;
            border: 1px solid #fbbf24;
            border-radius: 12px;
            padding: 16px;
            margin: 24px 0;
            text-align: left;
        }
        .data-info-title {
            font-size: 14px;
            color: #92400e;
            font-weight: 600;
            margin: 0 0 8px;
        }
        .data-list {
            font-size: 14px;
            color: #92400e;
            line-height: 1.5;
            margin: 0;
            padding-left: 20px;
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
            <div class="success-indicator">🗑️</div>
            
            <div class="email-content">
                <p class="greeting">Hei ${displayName},</p>
                <h1 class="title">Bekreft sletting av konto</h1>
                
                <div class="warning-notice">
                    <div class="warning-icon">!</div>
                    <p class="warning-text"><strong>PERMANENT HANDLING:</strong> Du er i ferd med å permanent slette din ${EMAIL_CONSTANTS.COMPANY.NAME}-konto og alle tilknyttede data. Denne handlingen kan ikke angres.</p>
                </div>
                
                <p class="description">Du ba om å slette kontoen din hos ${EMAIL_CONSTANTS.COMPANY.NAME}. Hvis du er sikker på at du vil fortsette, klikk på knappen nedenfor for å verifisere og permanent slette kontoen din.</p>
                
                <div class="data-info">
                    <p class="data-info-title">📋 Data som blir permanent slettet:</p>
                    <ul class="data-list">
                        <li>Din profilinformasjon</li>
                        <li>Kontoinnstillinger og preferanser</li>
                        <li>Autentiseringsdata og sesjoner</li>
                        <li>Alle tilknyttede applikasjonsdata</li>
                    </ul>
                </div>
                
                <div class="button-container">
                    <a href="${verificationUrl}" class="delete-button">Verifiser og slett min konto</a>
                </div>
                
                <div class="security-notice">
                    <div class="info-icon">i</div>
                    <p class="security-text">Hvis du ikke ba om denne slettingen av kontoen, kan du ignorere denne e-posten og kontakte oss umiddelbart. Kontoen din forblir aktiv og uendret.</p>
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

⚠️ PERMANENT HANDLING: Du er i ferd med å permanent slette din ${EMAIL_CONSTANTS.COMPANY.NAME}-konto og alle tilknyttede data. Denne handlingen kan ikke angres.

Du ba om å slette kontoen din hos ${EMAIL_CONSTANTS.COMPANY.NAME}. Hvis du er sikker på at du vil fortsette, bruk lenken nedenfor for å verifisere og permanent slette kontoen din.

Verifiseringslenke: ${verificationUrl}

📋 Data som blir permanent slettet:
- Din profilinformasjon
- Kontoinnstillinger og preferanser
- Autentiseringsdata og sesjoner
- Alle tilknyttede applikasjonsdata

🛡️ Sikkerhetsnotis: Hvis du ikke ba om denne slettingen av kontoen, kan du ignorere denne e-posten og kontakte oss umiddelbart. Kontoen din forblir aktiv og uendret.

---
${EMAIL_CONSTANTS.COMPANY.NAME}
Sikre autentiseringstjenester

Denne e-posten ble sendt til ${userEmail}.
  `;

  return { subject, html, text };
}
