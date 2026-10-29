import { EMAIL_CONSTANTS } from './email-constants';

export const footer = () => `
  <style>
    .footer {
      background: transparent;
      padding: 32px 24px;
      margin-top: 24px;
    }
    .contact-grid {
      display: grid;
      grid-template-columns: 1fr 1fr;
      gap: 32px;
      margin-bottom: 24px;
    }
    .contact-column {
      text-align: left;
    }
    .contact-label {
      font-size: 12px;
      font-weight: 700;
      color: #64748b;
      text-transform: uppercase;
      letter-spacing: 1px;
      margin: 0 0 12px;
    }
    .contact-info {
      font-size: 14px;
      color: #1e293b;
      line-height: 1.6;
      margin: 0;
      font-weight: 400;
    }
    .copyright {
      text-align: center;
      padding-top: 24px;
      border-top: 1px solid #e2e8f0;
    }
    .footer-text {
      font-size: 12px;
      color: #64748b;
      margin: 0;
    }
    @media (max-width: 480px) {
      .contact-grid {
        grid-template-columns: 1fr;
        gap: 24px;
      }
    }
  </style>
  <div class="footer">
    <div class="contact-grid">
      <div class="contact-column">
        <h3 class="contact-label">SUPPORT</h3>
        <p class="contact-info">${EMAIL_CONSTANTS.COMPANY.NAME} Support<br>
        ${EMAIL_CONSTANTS.COMPANY.ADDRESS.STREET}, ${EMAIL_CONSTANTS.COMPANY.ADDRESS.POSTAL_CODE} ${EMAIL_CONSTANTS.COMPANY.ADDRESS.CITY}<br>
        ${EMAIL_CONSTANTS.COMPANY.ADDRESS.REGION}, ${EMAIL_CONSTANTS.COMPANY.ADDRESS.COUNTRY}</p>
      </div>
      
      <div class="contact-column">
        <h3 class="contact-label">KONTAKT</h3>
        <p class="contact-info">${EMAIL_CONSTANTS.COMPANY.SUPPORT_EMAIL}<br>
        ${EMAIL_CONSTANTS.COMPANY.PHONE}</p>
      </div>
    </div>
    
    <div class="copyright">
      <p class="footer-text">© ${new Date().getFullYear()} ${EMAIL_CONSTANTS.COMPANY.NAME}. Alle rettigheter reservert.</p>
    </div>
  </div>
`;
