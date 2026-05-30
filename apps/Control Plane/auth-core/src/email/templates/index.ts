export { EMAIL_CONSTANTS } from './email-constants';

export { header } from './header';
export { footer } from './footer';

export {
  generateEmailVerificationTemplate,
  type EmailVerificationTemplateProps,
} from './email-verification.template';

export {
  generatePasswordResetTemplate,
  type PasswordResetTemplateProps,
} from './password-reset.template';

export {
  generateOtpTemplate,
  type OtpTemplateProps,
  emailOtpEmail,
  type EmailOtpParams,
} from './otp.template';

export {
  generateEmailChangeTemplate,
  type EmailChangeTemplateProps,
} from './email-change.template';

export {
  generateAccountDeletionTemplate,
  type AccountDeletionTemplateProps,
} from './account-deletion.template';
