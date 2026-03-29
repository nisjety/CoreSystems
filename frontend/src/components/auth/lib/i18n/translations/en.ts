import type { TranslationDict } from '../index';

/**
 * English translations
 * Complete coverage of all authentication features
 */
export const englishTranslations: TranslationDict = {
  common: {
    required: 'Required',
    optional: 'Optional',
    loading: 'Loading...',
    error: 'Error',
    success: 'Success',
    cancel: 'Cancel',
    confirm: 'Confirm',
    close: 'Close',
    save: 'Save',
    delete: 'Delete',
    edit: 'Edit',
    back: 'Back',
    next: 'Next',
    previous: 'Previous',
    continue: 'Continue',
    yes: 'Yes',
  no: 'No',
  selected: 'selected',
  },

  auth: {
    fields: {
      name: 'Full name',
      email: 'Email address',
      password: 'Password',
      confirmPassword: 'Confirm password',
    },

    actions: {
      signin: 'Sign in',
      signup: 'Create account',
      signout: 'Sign out',
      forgotPassword: 'Send reset link',
      resetPassword: 'Reset password',
      loading: 'Processing...',
      success: 'Operation completed',
      error: 'An error occurred',
    },

    modes: {
      signin: {
        title: 'Sign in',
        description: 'Welcome back! Sign in to your account.',
        action: 'Sign in to your account',
      },
      signup: {
        title: 'Create account',
        description: 'Create a new account to get started.',
        action: 'Create a new account',
      },
      forgot: {
        title: 'Forgot password',
        description: 'Reset your password.',
        action: 'Reset your password',
      },
      default: {
        title: 'Authentication',
        description: 'Please select a sign-in method',
      },
    },

    sso: {
      title: 'Enterprise SSO',
      businessEmail: 'Business email',
      organizationDomain: 'Organization domain',
      continue: 'Continue with SSO',
      connecting: 'Connecting...',
      description: 'Enter your business email address to sign in with your organization\'s identity provider.',
      domainDescription: 'If you know your organization\'s domain, enter it here to skip automatic detection.',
      businessEmailRequired: 'Business email required for enterprise SSO',
      discoveryError: 'Could not discover SSO providers',
      noProvidersFound: 'No SSO providers found for this domain',
      redirecting: 'Redirecting to {{provider}}...',
      authError: 'Could not start SSO authentication',
      emailHelp: 'Enter your business email address',
      continueHelp: 'We\'ll find the right SSO provider for your domain',
      aboutSso: 'About Enterprise SSO',
      aboutDescription: 'Enterprise Single Sign-On allows you to log in with your organization\'s credentials',
      selectProvider: 'Select SSO provider',
      selectDescription: 'Found providers for {{domain}}',
      providersListLabel: 'Available SSO providers',
      defaultProviderDescription: 'Enterprise identity provider',
      disabledProvidersNotice: 'Some providers are disabled. Contact your IT department.',
      authenticating: 'Authenticating',
      authenticatingDescription: 'Redirecting to {{provider}}',
      checking: 'Checking SSO...',
      domainTitle: 'SSO for {{domain}}',
    },

    organization: {
      name: 'Organization name',
      slug: 'Organization URL',
      inviteEmail: 'Invite team member',
      inviteRole: 'Role for invited member',
      create: 'Create organization',
      creating: 'Creating organization...',
      nameDescription: 'The display name for your organization.',
      slugDescription: 'A unique identifier for the organization (lowercase, no spaces).',
      inviteDescription: 'Email address of someone you want to invite to the organization.',
    },

    organizationManagement: {
      title: 'Your organizations',
      description: 'Manage organization memberships and create new ones',
      createNew: 'Create new organization',
      createTitle: 'Create organization',
      createDescription: 'Create a new organization to manage your team',
      manageTitle: 'Manage members and invitations',
      noOrganizations: 'No organizations yet',
      noOrganizationsDescription: 'Create an organization to start collaborating with your team',
      
      // Form labels
      nameLabel: 'Organization name',
      slugLabel: 'Organization slug',
      slugHelper: 'Used in URLs',
      
      // Actions
      inviteUser: 'Invite new member',
      sendInvitation: 'Send invitation',
      sending: 'Inviting...',
      copyLink: 'Copy invitation link',
      deleteInvitation: 'Delete invitation',
      
      // Invitations
      pendingInvitations: 'Pending invitations',
      expires: 'Expires',
      
      // OAuth
      oauthTitle: 'Register OAuth application',
      appNameLabel: 'Application name',
      redirectUrlLabel: 'Redirect URL',
      registerApp: 'Register application',
      
      // Statistics
      members: 'members',
      member: 'member',
      
      // Messages
      createSuccess: 'Organization created',
      inviteSuccess: 'Invitation sent',
      linkCopied: 'Link copied',
      inviteDeleted: 'Invitation deleted',
      
      // Errors
      createError: 'Could not create organization',
      inviteError: 'Could not send invitation',
      deleteError: 'Could not delete invitation',
      copyError: 'Could not copy invitation link',
      nameRequired: 'Organization name and slug are required',
      inviteRequired: 'Email and organization selection are required',
    },

    placeholders: {
      name: 'John Doe',
      email: 'name@example.com',
      password: '••••••••',
      ssoEmail: 'you@company.com',
      domain: 'company.com',
      orgName: 'Acme Inc',
      orgSlug: 'acme-inc',
      inviteEmail: 'colleague@company.com',
    },

    roles: {
      member: 'Member',
      admin: 'Administrator',
      owner: 'Owner',
      guest: 'Guest',
    },

    messages: {
      emailSent: 'Email sent! Check your inbox.',
      passwordReset: 'Your password has been reset.',
      accountCreated: 'Your account has been created.',
      loginSuccess: 'You are now signed in.',
      logoutSuccess: 'You have been signed out.',
      invalidCredentials: 'Invalid email or password.',
      emailExists: 'An account with this email already exists.',
      passwordMismatch: 'Passwords do not match.',
      weakPassword: 'Password is too weak. Use at least 8 characters.',
      networkError: 'Network error. Please try again.',
    },

    validation: {
      emailRequired: 'Email address is required',
      emailInvalid: 'Invalid email address',
      passwordRequired: 'Password is required',
      passwordMinLength: 'Password must be at least 8 characters long',
      passwordMismatch: 'Passwords do not match',
      nameRequired: 'Name is required',
      nameMinLength: 'Name must be at least 2 characters long',
      orgNameRequired: 'Organization name is required',
      orgSlugRequired: 'Organization URL is required',
      orgSlugInvalid: 'URL can only contain lowercase letters, numbers, and hyphens',
      emailTooLong: 'Email address is too long (max 254 characters)',
      passwordUpper: 'Password must contain uppercase letters',
      passwordLower: 'Password must contain lowercase letters',
      passwordNumber: 'Password must contain numbers',
      passwordSpecial: 'Password must contain special characters',
      confirmPasswordRequired: 'Confirm password is required',
      nameInvalidChars: 'Name contains invalid characters',
    },

    passwordStrength: {
      veryWeak: 'Very weak',
      weak: 'Weak',
      ok: 'Fair',
      strong: 'Strong',
      veryStrong: 'Very strong',
      enterPassword: 'Enter password',
      missing: 'Missing',
    },

    guards: {
      checkingAuthTitle: 'Confirming access...',
      checkingAuthMessage: 'Please wait while we check your authentication status.',
      authErrorTitle: 'Authentication Error',
      authErrorMessage: 'An error occurred while verifying your authentication. Please try again.',
      retry: 'Retry',
      accessDeniedTitle: 'Access Denied',
      accessDeniedMessage: 'You do not have permission to access this resource.',
      goToLogin: 'Go to login',
      goBack: 'Go back',
      emailMustBeVerified: 'Email must be verified before access is granted',
      twoFactorRequired: 'Two-factor authentication is required',
      roleRequired: 'Role "{{role}}" is required',
      permissionsMissing: 'Missing required permissions: {{perms}}',
      organizationAccessRequired: 'Access to organization "{{org}}" required',
      customValidationFailed: 'Custom validation failed',
      customValidationError: 'Custom validation error',
      authRequired: 'Authentication required',
      inactiveAccount: 'Account is inactive',
      timeRestricted: 'Access not allowed at this time',
      organizationRoleRequired: 'Required organization role(s): {{roles}}',
      organizationPermissionsRequired: 'Required organization permission(s): {{perms}}',
      roleRequiredGlobal: 'Required role(s): {{roles}}',
      permissionsRequiredGlobal: 'Required permission(s): {{perms}}',
      roleValidationError: 'Role validation error',
      errorDetails: 'Error details',
      authStatus: 'Auth Status',
      loading: 'Loading:',
      authenticated: 'Authenticated:',
      user: 'User:',
      emailVerified: 'Email verified:',
      twoFactor: '2FA:',
      role: 'Role:',
      tryAgain: 'Try again',
      currentRole: 'Current role',
      organization: 'Organization',
      validatingPermissions: 'Checking permissions...',
      requiredRoles: 'Required roles: {{roles}}',
      validationErrorTitle: 'Validation Error',
      accessDeniedGeneric: 'You do not have the necessary permissions to access this resource.',
      errorCheckingPermissions: 'An error occurred while checking your permissions.',
      errorDetailsLabel: 'Error details',
      roleStatus: 'Role status',
      rolesLabel: 'Roles',
      permissionsLabel: 'Permissions',
      organizationsLabel: 'Organizations',
      organizationsHeader: 'Organizations:',
    },

    tabs: {
      signin: { label: 'Sign in', shortLabel: 'Sign in', description: 'Welcome back! Sign in to your account.' },
      signup: { label: 'Register', shortLabel: 'Sign up', description: 'Create a new account to get started.' },
      enterpriseSso: { label: 'SSO', shortLabel: 'SSO', description: 'Enter your business email address to sign in with your organization\'s identity provider.' },
      org: { label: 'Org', shortLabel: 'Org', description: 'Organization name' },
    },

    navigation: {
      authTabsLabel: 'Authentication options',
      authOptionsLabel: 'Authentication choices',
    },

    security: {
      title: 'Enterprise-Grade Security',
      mfa: { title: 'Multi-Factor Authentication', description: 'Secure your account with email OTP, SMS verification, or TOTP authenticator apps.' },
      passwordless: { title: 'Passwordless Authentication', description: 'Use passkeys and WebAuthn for secure, convenient authentication without passwords.' },
      sso: { title: 'Enterprise SSO', description: 'Seamless integration with your organization\'s identity provider and directory services.' },
      gdpr: { title: 'GDPR Compliance', description: 'Built-in privacy controls and consent management for regulatory compliance.' },
      notifications: {
        securitySettingsUpdated: 'Security settings updated',
        securityPreferencesUpdated: 'Security preferences updated',
        settingsReset: 'Settings reset to defaults',
        deviceTrusted: 'Device trusted',
        deviceUntrusted: 'Device untrusted',
        deviceRemoved: 'Device removed',
        deviceRenamed: 'Device renamed',
        sessionTerminated: 'Session terminated',
        sessionsTerminated: 'All sessions terminated',
        sessionExtended: 'Session extended',
        passwordChanged: 'Password changed successfully',
        trustedIpAdded: 'IP address added to whitelist',
        trustedIpRemoved: 'IP address removed from whitelist',
        dataExported: 'Data exported successfully',
        auditLogDownloadStarted: 'Audit log download started'
      }
    },

    totp: {
      loading: { title: 'Setting up authenticator', description: 'Please wait while we prepare your TOTP secret' },
      error: { title: 'Setup failed', description: 'Unable to set up authenticator. Please try again.', unexpected: 'An unexpected error occurred', retry: 'Retry', cancel: 'Cancel' },
      setup: {
        title: 'Set up authenticator app', description: 'Scan the QR code with your authenticator app or enter the secret key manually', step1Title: 'Configure your authenticator app', copyQrUrl: 'Copy QR URL', copied: 'Copied!',
        manualEntry: 'Manual entry', secretLabel: 'Secret key', instructions: 'Open your authenticator app (Google Authenticator, Authy, etc.) and either scan the QR code or manually enter the secret key to add this account.', continue: 'Continue', showSecret: 'Show secret key', hideSecret: 'Hide secret key', copySecretAria: 'Copy secret key to clipboard', copyQrUrlAria: 'Copy QR code URL to clipboard'
      },
      verify: {
        title: 'Verify your setup', description: 'Enter the 6-digit code from your authenticator app to complete setup', step2Title: 'Verify authenticator code', codeLabel: 'Verification code', codeHelp: 'Enter the 6-digit code shown in your authenticator app', invalidCode: 'Invalid verification code. Please try again.', verifying: 'Verifying...', complete: 'Complete setup', back: 'Back', cancel: 'Cancel'
      }
    },

    page: {
      terms: {
        prefixSignin: 'By continuing you accept our', and: 'and', termsOfUse: 'terms of use', privacyPolicy: 'privacy policy', dataNotice: 'We store only essential sign-in data on this device. It expires automatically.', deleteCookie: 'Clear local data'
      },
      support: {
        needHelp: 'Need help?', contactSupport: 'Contact Support', helpLabel: 'Help'
      }
    },

    twoFactor: {
      title: 'Two-Factor Authentication',
      titleLogin: 'Two-Factor Authentication',
      descriptionLogin: 'Complete your login by verifying your identity',
      descriptionGeneric: 'Verify your identity to continue',
      methods: { totp: 'Authenticator App', email: 'Email Code', sms: 'SMS Code', recovery: 'Recovery Code' },
      badges: { totp: 'Authenticator App', email: 'Email Verification', sms: 'SMS Verification', recovery: 'Recovery Code' },
      instructions: {
        totp: 'Open your authenticator app and enter the 6-digit code',
        emailSendTo: "We'll send a code to {{email}}",
        smsSendTo: "We'll send a code to {{phone}}",
        recovery: 'Enter one of your backup recovery codes'
      },
      labels: {
        verificationCode: 'Verification Code',
        emailVerificationCode: 'Email Verification Code',
        smsVerificationCode: 'SMS Verification Code',
        recoveryCode: 'Recovery Code'
      },
      actions: {
        sendEmail: 'Send Email Code',
        sendSms: 'Send SMS Code',
        verifyCode: 'Verify Code',
        verifyEmailCode: 'Verify Email Code',
        verifySmsCode: 'Verify SMS Code',
        useRecoveryCode: 'Use Recovery Code',
        back: 'Back'
      },
      status: { sending: 'Sending...', verifying: 'Verifying...' },
      success: { emailCodeSent: 'Verification code sent to your email', smsCodeSent: 'Verification code sent to your phone' },
      errors: { invalidCode: 'Invalid verification code. Please try again.', sendEmailFailed: 'Failed to send email code. Please try again.', sendSmsFailed: 'Failed to send SMS code. Please try again.', invalidRecoveryCode: 'Invalid recovery code. Please try again.' },
      management: {
        headerDescription: 'Secure your account with additional verification methods',
        active: 'Active', inactive: 'Inactive',
        enablePrompt: { title: 'Enable Two-Factor Authentication', description: 'Protect your account by enabling at least one 2FA method. We recommend starting with an authenticator app for the highest security.' },
        availableMethods: { title: 'Available Methods', description: 'Choose which verification methods you want to enable for your account' },
        methodDescriptions: { totp: 'Use an authenticator app like Google Authenticator or Authy', email: 'Receive verification codes via email', sms: 'Receive verification codes via SMS', default: 'Additional security method' },
        method: { enabled: 'Enabled', lastUsedPrefix: 'Last used: ', setUp: 'Set up' },
        addAuthenticator: { title: 'Add Authenticator App', description: 'The most secure 2FA method. Works offline and generates codes every 30 seconds.', action: 'Set up authenticator' },
        toggleError: 'Failed to update 2FA method. Please try again.',
        recovery: {
          title: 'Recovery Codes', description: 'Backup codes to access your account if you lose access to your 2FA methods', introTitle: 'Backup Recovery Codes', introDescription: 'Generate and safely store backup codes to access your account if you lose your 2FA device.',
          viewCodes: 'View codes', generateNew: 'Generate new', hide: 'Hide', loading: 'Loading recovery codes...', yourCodes: 'Your Recovery Codes',
          storeSafelyTitle: 'Store these codes safely', storeSafelyDescription: 'Each code can only be used once. Save them in a secure location like a password manager.',
          copyCodes: 'Copy codes', copied: 'Copied!', loadError: 'Failed to load recovery codes. Please try again.', regenerateError: 'Failed to regenerate recovery codes. Please try again.',
          confirmRegenerate: 'Are you sure? This will invalidate all existing recovery codes.'
        },
        tips: { title: 'Security Best Practices', tip1: 'Enable multiple 2FA methods for redundancy in case you lose access to one.', tip2: 'Store your recovery codes in a secure location separate from your devices.', tip3: 'Use authenticator apps rather than SMS when possible for better security.', tip4: 'Regularly review and update your 2FA settings, especially after device changes.' }
      },
      recoveryCodes: {
        loadingTitle: 'Loading Recovery Codes', loadingDescription: 'Please wait while we load your recovery codes...',
        title: 'Recovery Codes', description: 'Backup codes to access your account when you can\'t use your 2FA device', remainingLabel: 'remaining', lowCodesBadge: 'Low codes', noCodesBadge: 'No codes left',
        lowCodesTitle: 'Running Low on Recovery Codes', lowCodesDescription: 'You only have {{count}} recovery code{{plural}} left. Consider generating new codes to ensure you can always access your account.',
        noCodesTitle: 'No Recovery Codes Available', noCodesDescription: 'You have used all your recovery codes. Generate new codes immediately to ensure you can access your account if you lose your 2FA device.',
        manageTitle: 'Manage Recovery Codes', manageDescription: 'View your current codes or generate new ones',
        viewCodes: 'View codes', hideCodes: 'Hide codes', generateCodes: 'Generate codes', generateNewCodes: 'Generate new codes', generating: 'Generating...', confirmRegenerate: 'Confirm regenerate', regenerate: 'Generate new codes', cancel: 'Cancel',
        warningTitle: 'Warning: This will invalidate all current codes', warningDescription: 'Regenerating will create new recovery codes and make all existing codes unusable. Make sure to save the new codes securely.',
        copyAll: 'Copy all', copied: 'Copied!', export: 'Export',
        headerYourCodes: 'Your Recovery Codes', headerCodesDescription: 'Each code can only be used once. Store them securely.',
        noCodesAvailableTitle: 'No recovery codes available', noCodesAvailableDescription: 'Generate recovery codes to have a backup way to access your account.',
        totalCodesLabel: 'Total codes', remaining: 'remaining',
        loadError: 'Failed to load recovery codes. Please try again.', regenerateError: 'Failed to regenerate recovery codes. Please try again.',
        exportFile: {
          title: 'Emergency Recovery Codes', intro1: 'These codes can be used to access your account if you lose access to your two-factor authentication device.', intro2: 'Each code can only be used once. Store them in a safe place.', generatedLabel: 'Generated:', codesHeader: 'Recovery Codes:', notesHeader: 'IMPORTANT SECURITY NOTES:', note1: 'Each code can only be used once', note2: 'Store these codes in a secure location', note3: 'Do not share these codes with anyone', note4: 'Generate new codes if you suspect they have been compromised', fileNamePrefix: 'recovery-codes'
        }
      },
      notifications: {
        setupComplete: 'Two-factor authentication enabled successfully',
        enableSuccess: 'Two-factor authentication enabled',
        disableSuccess: 'Two-factor authentication disabled',
        methodEnabled: '{{method}} two-factor authentication enabled',
        methodDisabled: '{{method}} two-factor authentication disabled',
        primarySet: '{{method}} set as primary 2FA method',
        backupCodesGenerated: 'New backup codes generated',
        codeSentVia: 'Verification code sent via {{method}}',
        allDisabled: 'All two-factor authentication methods disabled',
        resetSuccess: 'Two-factor authentication reset successfully'
      }
    },
    emailVerification: {
      status: {
        verified: { title: 'Email verified', message: 'Your email address has been verified.' },
        pending: { title: 'Verification pending', message: 'Check your email and click the verification link.' },
        expired: { title: 'Verification expired', message: 'The verification link has expired. Please request a new one.' },
        failed: { title: 'Verification failed', message: 'There was a problem verifying your email. Please try again.' },
        notSent: { title: 'Verification not sent', message: 'Click below to send a verification email.' },
        loading: { title: 'Checking status', message: 'Please wait while we check your verification status.' },
        unknown: { title: 'Unknown status', message: 'Cannot determine verification status.' }
      },
      interface: {
        refreshStatus: 'Refresh status', emailLabel: 'Email:', sending: 'Sending...', resendIn: 'Resend in', sendVerification: 'Send verification', resend: 'Resend', lastSent: 'Last sent:', emailNotFound: 'Email not found?', checkSpam: '• Check spam/junk folder', checkCorrect: 'is correct', waitDelivery: '• Wait a few minutes for delivery'
      },
      badges: { verified: 'Verified', pending: 'Pending', expired: 'Failed', failed: 'Failed', notSent: 'Not sent', loading: 'Checking', unknown: 'Unknown' },
      help: { title: 'Why verify your email?', reason1: 'Secure your account with two-factor authentication', reason2: 'Receive important security notifications', reason3: 'Enable password reset functionality', reason4: 'Follow security best practices', needHelp: 'Need help?', supportGuide: 'Visit our support guide', supportUrl: '/support/email-verification' }
    },
  },

  modal: {
    auth: {
      close: 'Close modal',
      opened: 'Authentication modal opened',
      closed: 'Authentication modal closed',
      loading: {
        title: 'Authenticating',
        message: 'Processing authentication data...',
      },
      success: {
        login: 'Sign in successful!',
        register: 'Registration successful!',
        default: 'Authentication completed!',
      },
      error: {
        title: 'Authentication error',
        message: 'An error occurred during authentication. Please try again.',
        close: 'An error occurred while closing the modal.',
      },
      mode: {
        signin: {
          title: 'Sign in',
          description: 'Sign in to your account to continue',
        },
        signup: {
          title: 'Create account',
          description: 'Create a new account to get started',
        },
        forgot: {
          title: 'Reset password',
          description: 'Enter your email address to reset your password',
        },
        verify: {
          title: 'Verify account',
          description: 'Verify your email address',
        },
        sso: {
          title: 'Enterprise SSO',
          description: 'Sign in with your organization\'s credentials',
        },
        organization: {
          title: 'Organization access',
          description: 'Access your organization dashboard',
        },
        default: {
          title: 'Authentication',
          description: 'Authenticate to continue',
        },
      },
    },

    confirmation: {
      opened: 'Confirmation modal opened',
      closed: 'Confirmation modal closed',
      cancelled: 'Action was cancelled',
      success: {
        action: 'Action completed successfully!',
        deleted: 'Item was deleted',
        confirmed: 'Action was confirmed',
        logout: 'You were signed out',
        permission: 'Permissions were updated',
        cookies: 'Cookies were deleted',
      },
      error: {
        action: 'An error occurred during execution',
        network: 'Network error - please try again',
        permission: 'You do not have permission for this action',
      },
    },
  },

  consent: {
    banner: {
      message: 'By clicking "Accept", you agree to the storage of cookies on your device.',
      messageMobile: 'By clicking "Accept", you agree to cookie storage.',
      settings: 'Settings',
      reject: 'Reject',
      accept: 'Accept',
      settingsLabel: 'Open privacy settings',
    },

    preferences: {
      title: 'Privacy Preferences',
      close: 'Close',
      intro: 'When you visit a website, it may store or retrieve information on your browser, mostly in the form of cookies. Some are necessary for the site to function, others help us improve your experience. This information usually does not directly identify you, but can provide a more personalized experience.',
      allowAll: 'Allow',
      rejectAll: 'Reject All',
      acceptAll: 'Accept All',
      saveChoices: 'Save Choices',
      showDetails: 'Show Details',
      hideDetails: 'Hide Details',
    },

    categories: {
      necessary: {
        title: 'Strictly Necessary Cookies',
        description: 'Necessary for basic functionality and cannot be turned off in our systems.',
      },
      performance: {
        title: 'Performance Cookies',
        description: 'Allow us to count visits and traffic sources to measure and improve performance. Help us know which pages are most and least popular and how visitors move around the site.',
      },
      functional: {
        title: 'Functional Cookies',
        description: 'Enable enhanced functionality and personalization. May be set by us or third-party services.',
      },
      marketing: {
        title: 'Targeting/Marketing',
        description: 'Used to build an interest profile and show relevant ads on other websites.',
      },
    },

    actions: {
      accepted: 'Consent accepted',
      rejected: 'Consent rejected',
      settingsOpened: 'Privacy settings opened',
      allAccepted: 'All consents accepted',
      allRejected: 'All consents rejected',
      choicesSaved: 'Consent choices saved',
    },

    storage: {
      error: 'Could not save consent choices',
      success: 'Consent choices saved',
    },

    script: {
      loadError: 'Could not load script',
      loadSuccess: 'Script loaded',
    },

    validation: {
      error: 'Invalid consent data',
    },

    reset: {
      success: 'Consent settings reset',
    },
  },

  language: {
    current: 'Current language',
    select: 'Select language',
    norwegian: 'Norwegian',
    english: 'English',
    changed: 'Language was changed',
  },

  social: {
    orWith: 'or with',
    signInWith: 'Sign in with {{provider}}',
    unavailable: '{{provider}} (unavailable)',
    providers: {
      google: 'Google',
      microsoft: 'Microsoft',
      okta: 'Okta',
      vipps: 'Vipps',
    },
  },

  passkey: {
    title: 'Passkey',
    register: 'Create passkey',
    authenticate: 'Use passkey',
    manage: 'Manage passkeys',
    delete: 'Delete passkey',
    creating: 'Creating passkey...',
    authenticating: 'Authenticating...',
    deleting: 'Deleting...',
    unsupported: 'Passkeys not supported',
    unsupportedMessage: 'Your browser does not support passkeys. Please use a modern browser or try password authentication.',
    emailRequired: 'Enter an email to create a passkey',
    description: 'Use a passkey for faster, safer sign-in',
    quickAuth: 'Use passkey',
    noPasskeys: 'No passkeys registered',
    noPasskeysDescription: 'Add a passkey for faster and more secure authentication',
    deleteConfirm: 'Are you sure you want to delete this passkey?',
    lastUsed: 'Last used',
    created: 'Created',
    errors: {
      registrationFailed: 'Could not register passkey',
      authenticationFailed: 'Could not authenticate with passkey',
      deleteFailed: 'Could not delete passkey',
    },
  },

  callback: {
    title: 'Completing sign in',
    processing: 'Please wait while we process your authentication.',
    success: 'Sign in successful!',
    redirecting: 'Redirecting to your dashboard...',
    error: 'Authentication failed. Please try again.',
    redirectingToSignIn: 'Redirecting to sign in page...',
    oauth: {
      error: 'OAuth authentication failed',
      cancelled: 'Authentication was cancelled',
      denied: 'Access was denied',
      timeout: 'Authentication timed out',
    },
  },

  // Dashboard translations
  dashboard: {
    aquatiqCard: {
      publishedBy: 'Published by',
      readMore: 'Read more',
      viewAllCollaborators: 'View all collaborators',
      noUpdates: 'No updates available',
      categories: {
        announcement: 'Announcement',
        event: 'Event',
        product: 'Product',
        business: 'Business',
      },
    },
    weather: {
      title: 'Weather',
      humidity: 'Humidity',
      wind: 'Wind',
      pressure: 'Pressure',
      noData: 'Could not load weather data',
    },
    news: {
      title: 'News',
      noNews: 'No news available',
    },
    traffic: {
      title: 'Traffic',
      noData: 'No traffic data available',
    },
  },
};
