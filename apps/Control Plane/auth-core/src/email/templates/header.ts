import { EMAIL_CONSTANTS } from './email-constants';

export const header = () => `
  <style>
    .email-header {
      display: flex;
      align-items: center;
      justify-content: flex-start;
      gap: 16px;
      padding: 32px 24px;
      background: transparent;
    }
    .brand-icon {
      width: 48px;
      height: 48px;
      background: ${EMAIL_CONSTANTS.BRANDING.PRIMARY_COLOR};
      border-radius: 12px;
      display: flex;
      align-items: center;
      justify-content: center;
      font-size: 20px;
      font-weight: 700;
      color: white;
    }
    .brand-name {
      font-size: 24px;
      font-weight: 700;
      color: ${EMAIL_CONSTANTS.BRANDING.PRIMARY_COLOR};
      margin: 0;
    }
    @media (max-width: 480px) {
      .email-header {
        padding: 20px;
      }
      .brand-name {
        font-size: 20px;
      }
    }
  </style>
  <div class="email-header">
    <div class="brand-icon">${EMAIL_CONSTANTS.BRANDING.LOGO_INITIALS}</div>
    <h1 class="brand-name">${EMAIL_CONSTANTS.COMPANY.NAME}</h1>
  </div>
`;
