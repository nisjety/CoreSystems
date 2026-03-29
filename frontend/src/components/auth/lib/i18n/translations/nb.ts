import type { TranslationDict } from '../index';

/**
 * Norwegian (Bokmål) translations
 * Complete coverage of all authentication features
 */
export const norwegianTranslations: TranslationDict = {
  common: {
    required: 'Påkrevd',
    optional: 'Valgfritt',
    loading: 'Laster...',
    error: 'Feil',
    success: 'Suksess',
    cancel: 'Avbryt',
    confirm: 'Bekreft',
    close: 'Lukk',
    save: 'Lagre',
    delete: 'Slett',
    edit: 'Rediger',
    back: 'Tilbake',
    next: 'Neste',
    previous: 'Forrige',
    continue: 'Fortsett',
    yes: 'Ja',
  no: 'Nei',
  selected: 'valgt',
  },

  auth: {
    fields: {
      name: 'Fullt navn',
      email: 'E-postadresse',
      password: 'Passord',
      confirmPassword: 'Bekreft passord',
    },

    actions: {
      signin: 'Logg inn',
      signup: 'Opprett konto',
      signout: 'Logg ut',
      forgotPassword: 'Send tilbakestillingslenke',
      resetPassword: 'Tilbakestill passord',
      loading: 'Behandler...',
  success: 'Vellykket',
      error: 'Det oppstod en feil',
    },

    modes: {
      signin: {
        title: 'Logg inn',
        description: 'Velkommen tilbake! Logg inn på kontoen din.',
        action: 'Logg inn på kontoen din',
      },
      signup: {
        title: 'Opprett konto',
        description: 'Opprett en ny konto for å komme i gang.',
        action: 'Opprett en ny konto',
      },
      forgot: {
        title: 'Glemt passord',
        description: 'Tilbakestill passordet ditt.',
        action: 'Tilbakestill passordet ditt',
      },
      default: {
        title: 'Autentisering',
        description: 'Vennligst velg innloggingsmetode',
      },
    },

    sso: {
      businessEmail: 'Arbeids-e-post',
      organizationDomain: 'Organisasjonsdomene',
      continue: 'Fortsett med SSO',
      connecting: 'Kobler til...',
      description: 'Skriv inn din arbeids-e-postadresse for å logge inn med organisasjonens identitetsleverandør.',
      domainDescription: 'Hvis du kjenner organisasjonens domene, skriv det inn her for å hoppe over automatisk deteksjon.',
      title: 'Enterprise SSO',
      businessEmailRequired: 'Arbeids-e-post kreves for enterprise SSO',
      discoveryError: 'Kunne ikke finne SSO-leverandører',
      noProvidersFound: 'Ingen SSO-leverandører funnet for domenet',
      redirecting: 'Omdirigerer til {{provider}}...',
      authError: 'Kunne ikke starte SSO-autentisering',
      emailHelp: 'Skriv inn arbeids-e-postadressen din',
      continueHelp: 'Vi vil finne riktig SSO-leverandør for domenet ditt',
      aboutSso: 'Om Enterprise SSO',
      aboutDescription: 'Enterprise Single Sign-On lar deg logge inn med organisasjonens legitimasjon',
      selectProvider: 'Velg SSO-leverandør',
      selectDescription: 'Fant leverandører for {{domain}}',
      providersListLabel: 'Tilgjengelige SSO-leverandører',
      defaultProviderDescription: 'Enterprise identitetsleverandør',
      disabledProvidersNotice: 'Noen leverandører er deaktivert. Kontakt IT-avdelingen.',
      authenticating: 'Autentiserer',
      authenticatingDescription: 'Omdirigerer til {{provider}}',
      checking: 'Sjekker SSO...',
      domainTitle: 'SSO for {{domain}}',
    },

    organization: {
      name: 'Organisasjonsnavn',
      slug: 'Organisasjons-URL',
      inviteEmail: 'Inviter teammedlem',
      inviteRole: 'Rolle for invitert medlem',
      create: 'Opprett organisasjon',
      creating: 'Oppretter organisasjon...',
      nameDescription: 'Visningsnavnet for din organisasjon.',
      slugDescription: 'En unik identifikator for organisasjonen (små bokstaver, ingen mellomrom).',
      inviteDescription: 'E-postadresse til noen du vil invitere til organisasjonen.',
    },

    organizationManagement: {
      title: 'Dine organisasjoner',
      description: 'Administrer organisasjonsmedlemskap og opprett nye',
      createNew: 'Opprett ny organisasjon',
      createTitle: 'Opprett organisasjon',
      createDescription: 'Opprett en ny organisasjon for å administrere teamet ditt',
      manageTitle: 'Administrer medlemmer og invitasjoner',
      noOrganizations: 'Ingen organisasjoner ennå',
      noOrganizationsDescription: 'Opprett en organisasjon for å begynne samarbeidet med teamet ditt',
      
      // Form labels
      nameLabel: 'Organisasjonsnavn',
      slugLabel: 'Organisasjons-slug',
      slugHelper: 'Brukes i URL-er',
      
      // Actions
      inviteUser: 'Inviter nytt medlem',
      sendInvitation: 'Send invitasjon',
      sending: 'Inviterer...',
      copyLink: 'Kopier invitasjonslenke',
      deleteInvitation: 'Slett invitasjon',
      
      // Invitations
      pendingInvitations: 'Ventende invitasjoner',
      expires: 'Utløper',
      
      // OAuth
      oauthTitle: 'Registrer OAuth-applikasjon',
      appNameLabel: 'Applikasjonsnavn',
      redirectUrlLabel: 'Omdirigeringsurl',
      registerApp: 'Registrer applikasjon',
      
      // Statistics
      members: 'medlemmer',
      member: 'medlem',
      
      // Messages
      createSuccess: 'Organisasjon opprettet',
      inviteSuccess: 'Invitasjon sendt',
      linkCopied: 'Lenke kopiert',
      inviteDeleted: 'Invitasjon slettet',
      
      // Errors
      createError: 'Kunne ikke opprette organisasjon',
      inviteError: 'Kunne ikke sende invitasjon',
      deleteError: 'Kunne ikke slette invitasjon',
      copyError: 'Kunne ikke kopiere invitasjonslenke',
      nameRequired: 'Organisasjonsnavn og slug er påkrevd',
      inviteRequired: 'E-post og organisasjonsvalg er påkrevd',
    },

    placeholders: {
      name: 'Ola Nordmann',
      email: 'navn@eksempel.no',
      password: '••••••••',
      ssoEmail: 'deg@bedrift.no',
      domain: 'bedrift.no',
      orgName: 'Acme AS',
      orgSlug: 'acme-as',
      inviteEmail: 'kollega@bedrift.no',
    },

    roles: {
      member: 'Medlem',
      admin: 'Administrator',
      owner: 'Eier',
      guest: 'Gjest',
    },

    messages: {
      emailSent: 'E-post sendt! Sjekk innboksen din.',
      passwordReset: 'Passordet ditt har blitt tilbakestilt.',
      accountCreated: 'Kontoen din har blitt opprettet.',
      loginSuccess: 'Du er nå logget inn.',
      logoutSuccess: 'Du har blitt logget ut.',
      invalidCredentials: 'Ugyldig e-post eller passord.',
      emailExists: 'En konto med denne e-postadressen eksisterer allerede.',
      passwordMismatch: 'Passordene stemmer ikke overens.',
      weakPassword: 'Passordet er for svakt. Bruk minst 8 tegn.',
      networkError: 'Nettverksfeil. Vennligst prøv igjen.',
    },

    validation: {
      emailRequired: 'E-postadresse er påkrevd',
      emailInvalid: 'Ugyldig e-postadresse',
      passwordRequired: 'Passord er påkrevd',
      passwordMinLength: 'Passordet må være minst 8 tegn langt',
      passwordMismatch: 'Passordene stemmer ikke overens',
      nameRequired: 'Navn er påkrevd',
      nameMinLength: 'Navnet må være minst 2 tegn langt',
      orgNameRequired: 'Organisasjonsnavn er påkrevd',
      orgSlugRequired: 'Organisasjons-URL er påkrevd',
      orgSlugInvalid: 'URL kan kun inneholde små bokstaver, tall og bindestrek',
      emailTooLong: 'E-postadresse er for lang (maks 254 tegn)',
      passwordUpper: 'Passord må inneholde store bokstaver',
      passwordLower: 'Passord må inneholde små bokstaver',
      passwordNumber: 'Passord må inneholde tall',
      passwordSpecial: 'Passord må inneholde spesialtegn',
      confirmPasswordRequired: 'Bekreft passord er påkrevd',
      nameInvalidChars: 'Navn inneholder ugyldige tegn',
    },

    passwordStrength: {
      veryWeak: 'Svært svakt',
      weak: 'Svakt',
      ok: 'Greit',
      strong: 'Sterkt',
      veryStrong: 'Meget sterkt',
      enterPassword: 'Skriv inn passord',
      missing: 'Mangler',
    },

    guards: {
      checkingAuthTitle: 'Bekrefter tilgang...',
      checkingAuthMessage: 'Vennligst vent mens vi sjekker autentiseringsstatusen din.',
      authErrorTitle: 'Autentiseringsfeil',
      authErrorMessage: 'Det oppstod en feil under verifisering av autentiseringen din. Vennligst prøv igjen.',
      retry: 'Prøv igjen',
      accessDeniedTitle: 'Tilgang nektet',
      accessDeniedMessage: 'Du har ikke tillatelse til å få tilgang til denne ressursen.',
      goToLogin: 'Gå til innlogging',
      goBack: 'Gå tilbake',
      emailMustBeVerified: 'E-post må bekreftes før tilgang gis',
      twoFactorRequired: 'Tofaktor autentisering er påkrevd',
      roleRequired: 'Rollen "{{role}}" er påkrevd',
      permissionsMissing: 'Mangler nødvendige tillatelser: {{perms}}',
      organizationAccessRequired: 'Tilgang til organisasjon "{{org}}" kreves',
      customValidationFailed: 'Tilpasset validering feilet',
      customValidationError: 'Feil i tilpasset validering',
      authRequired: 'Autentisering kreves',
      inactiveAccount: 'Kontoen er inaktiv',
      timeRestricted: 'Tilgang ikke tillatt på dette tidspunktet',
      organizationRoleRequired: 'Nødvendig organisasjonsrolle: {{roles}}',
      organizationPermissionsRequired: 'Nødvendige organisasjonstillatelser: {{perms}}',
      roleRequiredGlobal: 'Nødvendig rolle: {{roles}}',
      permissionsRequiredGlobal: 'Nødvendige tillatelser: {{perms}}',
      roleValidationError: 'Feil i rollevalidering',
      errorDetails: 'Feildetaljer',
      authStatus: 'Auth Status',
      loading: 'Laster:',
      authenticated: 'Autentisert:',
      user: 'Bruker:',
      emailVerified: 'E-post bekreftet:',
      twoFactor: '2FA:',
      role: 'Rolle:',
      tryAgain: 'Prøv igjen',
      currentRole: 'Nåværende rolle',
      organization: 'Organisasjon',
      validatingPermissions: 'Sjekker tillatelser...',
      requiredRoles: 'Nødvendige roller: {{roles}}',
      validationErrorTitle: 'Valideringsfeil',
      accessDeniedGeneric: 'Du har ikke de nødvendige tillatelsene for å få tilgang til denne ressursen.',
      errorCheckingPermissions: 'Det oppstod en feil under sjekking av tillatelsene dine.',
      errorDetailsLabel: 'Feildetaljer',
      roleStatus: 'Rollestatus',
      rolesLabel: 'Roller',
      permissionsLabel: 'Tillatelser',
      organizationsLabel: 'Organisasjoner',
      organizationsHeader: 'Organisasjoner:',
    },

    tabs: {
      signin: { label: 'Logg inn', shortLabel: 'Logg inn', description: 'Velkommen tilbake! Logg inn på kontoen din.' },
      signup: { label: 'Registrer', shortLabel: 'Registrer', description: 'Opprett en ny konto for å komme i gang.' },
      enterpriseSso: { label: 'Arbeid', shortLabel: 'Arbeid', description: 'Skriv inn din arbeids-e-postadresse for å logge inn med organisasjonens identitetsleverandør.' },
      org: { label: 'Org', shortLabel: 'Org', description: 'Organisasjonsnavn' },
    },

    navigation: {
      authTabsLabel: 'Autentiseringsvalg',
      authOptionsLabel: 'Valg for autentisering',
    },

    security: {
      title: 'Bedriftsklasse Sikkerhet',
      mfa: { title: 'Tofaktor-autentisering', description: 'Sikre kontoen din med e-post OTP, SMS-verifisering eller TOTP-autentisering.' },
      passwordless: { title: 'Passordløs Autentisering', description: 'Bruk passkeys og WebAuthn for sikker, praktisk autentisering uten passord.' },
      sso: { title: 'Bedrift SSO', description: 'Sømløs integrasjon med organisasjonens identitetsleverandør og katalogtjenester.' },
      gdpr: { title: 'GDPR Samsvar', description: 'Innebygde personvernkontroller og samtykkehåndtering for regelverkssamsvar.' },
      notifications: {
        securitySettingsUpdated: 'Sikkerhetsinnstillinger oppdatert',
        securityPreferencesUpdated: 'Sikkerhetspreferanser oppdatert',
        settingsReset: 'Innstillinger tilbakestilt til standard',
        deviceTrusted: 'Enhet klarert',
        deviceUntrusted: 'Enhet ikke lenger klarert',
        deviceRemoved: 'Enhet fjernet',
        deviceRenamed: 'Enhet omdøpt',
        sessionTerminated: 'Økt avsluttet',
        sessionsTerminated: 'Alle økter avsluttet',
        sessionExtended: 'Økt forlenget',
        passwordChanged: 'Passord endret',
        trustedIpAdded: 'IP-adresse lagt til hvitliste',
        trustedIpRemoved: 'IP-adresse fjernet fra hvitliste',
        dataExported: 'Data eksportert',
        auditLogDownloadStarted: 'Nedlasting av revisjonslogg startet'
      }
    },

    totp: {
      loading: { title: 'Setter opp autentisering', description: 'Vennligst vent mens vi forbereder din TOTP-hemmelighet' },
      error: { title: 'Oppsett mislyktes', description: 'Kunne ikke sette opp autentisering. Prøv igjen.', unexpected: 'En uventet feil oppstod', retry: 'Prøv igjen', cancel: 'Avbryt' },
      setup: {
        title: 'Sett opp autentiseringsapp', description: 'Skann QR-koden med autentiseringsappen din eller skriv inn den hemmelige nøkkelen manuelt', step1Title: 'Konfigurer autentiseringsappen din', copyQrUrl: 'Kopier QR-URL', copied: 'Kopiert!',
        manualEntry: 'Manuell innskriving', secretLabel: 'Hemmelig nøkkel', instructions: 'Åpne autentiseringsappen din (Google Authenticator, Authy, etc.) og enten skann QR-koden eller skriv inn den hemmelige nøkkelen manuelt for å legge til denne kontoen.', continue: 'Fortsett', showSecret: 'Vis hemmelig nøkkel', hideSecret: 'Skjul hemmelig nøkkel', copySecretAria: 'Kopier hemmelig nøkkel til utklippstavle', copyQrUrlAria: 'Kopier QR-kode URL til utklippstavle'
      },
      verify: {
        title: 'Bekreft oppsettet ditt', description: 'Skriv inn den 6-sifrede koden fra autentiseringsappen din for å fullføre oppsettet', step2Title: 'Bekreft autentiseringskode', codeLabel: 'Bekreftelseskode', codeHelp: 'Skriv inn den 6-sifrede koden som vises i autentiseringsappen din', invalidCode: 'Ugyldig bekreftelseskode. Prøv igjen.', verifying: 'Bekrefter...', complete: 'Fullfør oppsett', back: 'Tilbake', cancel: 'Avbryt'
      }
    },

    page: {
      terms: {
        prefixSignin: 'Ved å fortsette aksepterer du våre', and: 'og', termsOfUse: 'brukervilkår', privacyPolicy: 'personvernregler', dataNotice: 'Vi lagrer kun nødvendig innloggingsdata på denne enheten. Dataen utløper automatisk.', deleteCookie: 'Slett lokale data'
      },
      support: {
        needHelp: 'Trenger du hjelp?', contactSupport: 'Kontakt Support', helpLabel: 'Hjelp'
      }
    },

    twoFactor: {
      title: 'Tofaktor-autentisering',
      titleLogin: 'Tofaktor-autentisering',
      descriptionLogin: 'Fullfør innloggingen din ved å bekrefte identiteten din',
      descriptionGeneric: 'Bekreft identiteten din for å fortsette',
      methods: { totp: 'Autentiseringsapp', email: 'E-postkode', sms: 'SMS-kode', recovery: 'Gjenopprettingskode' },
      badges: { totp: 'Autentiseringsapp', email: 'E-postbekreftelse', sms: 'SMS-bekreftelse', recovery: 'Gjenopprettingskode' },
      instructions: {
        totp: 'Åpne autentiseringsappen din og skriv inn den 6-sifrede koden',
        emailSendTo: 'Vi sender en kode til {{email}}',
        smsSendTo: 'Vi sender en kode til {{phone}}',
        recovery: 'Skriv inn en av dine reservegjenopprettingskoder'
      },
      labels: {
        verificationCode: 'Bekreftelseskode',
        emailVerificationCode: 'E-postbekreftelseskode',
        smsVerificationCode: 'SMS-bekreftelseskode',
        recoveryCode: 'Gjenopprettingskode'
      },
      actions: {
        sendEmail: 'Send e-postkode',
        sendSms: 'Send SMS-kode',
        verifyCode: 'Bekreft kode',
        verifyEmailCode: 'Bekreft e-postkode',
        verifySmsCode: 'Bekreft SMS-kode',
        useRecoveryCode: 'Bruk gjenopprettingskode',
        back: 'Tilbake'
      },
      status: { sending: 'Sender...', verifying: 'Bekrefter...' },
      success: { emailCodeSent: 'Bekreftelseskode sendt til e-posten din', smsCodeSent: 'Bekreftelseskode sendt til telefonen din' },
      errors: { invalidCode: 'Ugyldig bekreftelseskode. Prøv igjen.', sendEmailFailed: 'Kunne ikke sende e-postkode. Prøv igjen.', sendSmsFailed: 'Kunne ikke sende SMS-kode. Prøv igjen.', invalidRecoveryCode: 'Ugyldig gjenopprettingskode. Prøv igjen.' },
      management: {
        headerDescription: 'Sikre kontoen din med ekstra bekreftelsesmetoder',
        active: 'Aktiv', inactive: 'Inaktiv',
        enablePrompt: { title: 'Aktiver tofaktor-autentisering', description: 'Beskytt kontoen din ved å aktivere minst én 2FA-metode. Vi anbefaler å starte med en autentiseringsapp for høyest sikkerhet.' },
        availableMethods: { title: 'Tilgjengelige metoder', description: 'Velg hvilke bekreftelsesmetoder du vil aktivere for kontoen din' },
        methodDescriptions: { totp: 'Bruk en autentiseringsapp som Google Authenticator eller Authy', email: 'Motta bekreftelseskoder via e-post', sms: 'Motta bekreftelseskoder via SMS', default: 'Ekstra sikkerhetsmetode' },
        method: { enabled: 'Aktivert', lastUsedPrefix: 'Sist brukt: ', setUp: 'Sett opp' },
        addAuthenticator: { title: 'Legg til autentiseringsapp', description: 'Den sikreste 2FA-metoden. Fungerer offline og genererer koder hver 30. sekund.', action: 'Sett opp autentisering' },
        toggleError: 'Kunne ikke oppdatere 2FA-metode. Prøv igjen.',
        recovery: {
          title: 'Gjenopprettingskoder', description: 'Reserve-koder for å få tilgang til kontoen din hvis du mister tilgang til 2FA-metodene dine', introTitle: 'Reserve gjenopprettingskoder', introDescription: 'Generer og lagre gjenopprettingskoder trygt for å få tilgang til kontoen hvis du mister 2FA-enheten.',
          viewCodes: 'Vis koder', generateNew: 'Generer nye', hide: 'Skjul', loading: 'Laster gjenopprettingskoder...', yourCodes: 'Dine gjenopprettingskoder',
          storeSafelyTitle: 'Lagre disse kodene trygt', storeSafelyDescription: 'Hver kode kan bare brukes én gang. Lagre dem på et sikkert sted som en passordbehandler.',
          copyCodes: 'Kopier koder', copied: 'Kopiert!', loadError: 'Kunne ikke laste gjenopprettingskoder. Prøv igjen.', regenerateError: 'Kunne ikke regenerere gjenopprettingskoder. Prøv igjen.',
          confirmRegenerate: 'Er du sikker? Dette vil ugyldiggjøre alle eksisterende gjenopprettingskoder.'
        },
        tips: { title: 'Beste sikkerhetspraksis', tip1: 'Aktiver flere 2FA-metoder for redundans i tilfelle du mister tilgang til én.', tip2: 'Lagre gjenopprettingskodene dine på et sikkert sted, separat fra enhetene dine.', tip3: 'Bruk autentiseringsapper i stedet for SMS når mulig for bedre sikkerhet.', tip4: 'Gjennomgå og oppdater 2FA-innstillingene dine regelmessig, spesielt etter enhetsendringer.' }
      },
      recoveryCodes: {
        loadingTitle: 'Laster gjenopprettingskoder', loadingDescription: 'Vennligst vent mens vi laster gjenopprettingskodene dine...',
        title: 'Gjenopprettingskoder', description: 'Reserve-koder for å få tilgang til kontoen din når du ikke kan bruke 2FA-enheten din', remainingLabel: 'gjenstående', lowCodesBadge: 'Få koder', noCodesBadge: 'Ingen koder igjen',
        lowCodesTitle: 'Få gjenopprettingskoder igjen', lowCodesDescription: 'Du har bare {{count}} gjenopprettingskode{{plural}} igjen. Vurder å generere nye koder for å sikre at du alltid kan få tilgang til kontoen din.',
        noCodesTitle: 'Ingen gjenopprettingskoder tilgjengelig', noCodesDescription: 'Du har brukt alle gjenopprettingskodene dine. Generer nye koder umiddelbart for å sikre at du kan få tilgang til kontoen din hvis du mister 2FA-enheten din.',
        manageTitle: 'Administrer gjenopprettingskoder', manageDescription: 'Se dine nåværende koder eller generer nye',
        viewCodes: 'Vis koder', hideCodes: 'Skjul koder', generateCodes: 'Generer koder', generateNewCodes: 'Generer nye koder', generating: 'Genererer...', confirmRegenerate: 'Bekreft regenerering', regenerate: 'Generer nye koder', cancel: 'Avbryt',
        warningTitle: 'Advarsel: Dette vil ugyldiggjøre alle nåværende koder', warningDescription: 'Regenerering vil lage nye gjenopprettingskoder og gjøre alle eksisterende koder ubrukelige. Sørg for å lagre de nye kodene sikkert.',
        copyAll: 'Kopier alle', copied: 'Kopiert!', export: 'Eksporter',
        headerYourCodes: 'Dine gjenopprettingskoder', headerCodesDescription: 'Hver kode kan bare brukes én gang. Lagre dem sikkert.',
        noCodesAvailableTitle: 'Ingen gjenopprettingskoder tilgjengelig', noCodesAvailableDescription: 'Generer gjenopprettingskoder for å ha en reserve måte å få tilgang til kontoen din på.',
        totalCodesLabel: 'Totale koder', remaining: 'gjenstående',
        loadError: 'Kunne ikke laste gjenopprettingskoder. Prøv igjen.', regenerateError: 'Kunne ikke regenerere gjenopprettingskoder. Prøv igjen.',
        exportFile: {
          title: 'Nødgjenopprettingskoder', intro1: 'Disse kodene kan brukes for å få tilgang til kontoen din hvis du mister tilgang til tofaktor-autentiseringsenheten din.', intro2: 'Hver kode kan bare brukes én gang. Lagre dem på et sikkert sted.', generatedLabel: 'Generert:', codesHeader: 'Gjenopprettingskoder:', notesHeader: 'VIKTIGE SIKKERHETSMERKNADER:', note1: 'Hver kode kan bare brukes én gang', note2: 'Lagre disse kodene på et sikkert sted', note3: 'Ikke del disse kodene med noen', note4: 'Generer nye koder hvis du mistenker at de har blitt kompromittert', fileNamePrefix: 'gjenopprettingskoder'
        }
      },
      notifications: {
        setupComplete: 'Tofaktor-autentisering aktivert',
        enableSuccess: 'Tofaktor-autentisering aktivert',
        disableSuccess: 'Tofaktor-autentisering deaktivert',
        methodEnabled: '{{method}} tofaktor-autentisering aktivert',
        methodDisabled: '{{method}} tofaktor-autentisering deaktivert',
        primarySet: '{{method}} satt som primær 2FA-metode',
        backupCodesGenerated: 'Nye gjenopprettingskoder generert',
        codeSentVia: 'Bekreftelseskode sendt via {{method}}',
        allDisabled: 'Alle tofaktor-metoder deaktivert',
        resetSuccess: 'Tofaktor-autentisering tilbakestilt'
      }
    },
    emailVerification: {
      status: {
        verified: { title: 'E-post bekreftet', message: 'Din e-postadresse er bekreftet.' },
        pending: { title: 'Bekreftelse venter', message: 'Sjekk e-posten din og klikk på bekreftelseslenken.' },
        expired: { title: 'Bekreftelse utløpt', message: 'Bekreftelseslenken har utløpt. Vennligst be om en ny.' },
        failed: { title: 'Bekreftelse feilet', message: 'Det oppstod et problem med å bekrefte e-posten din. Prøv igjen.' },
        notSent: { title: 'Bekreftelse ikke sendt', message: 'Klikk nedenfor for å sende en bekreftelses-e-post.' },
        loading: { title: 'Sjekker status', message: 'Vennligst vent mens vi sjekker bekreftelsesstatusen din.' },
        unknown: { title: 'Ukjent status', message: 'Kan ikke bestemme bekreftelsesstatusen.' }
      },
      interface: {
        refreshStatus: 'Oppdater status', emailLabel: 'E-post:', sending: 'Sender...', resendIn: 'Send på nytt om', sendVerification: 'Send bekreftelse', resend: 'Send på nytt', lastSent: 'Sist sendt:', emailNotFound: 'Fant ikke e-posten?', checkSpam: '• Sjekk søppelpost/spam-mappen', checkCorrect: 'er riktig', waitDelivery: '• Vent noen minutter på levering'
      },
      badges: { verified: 'Bekreftet', pending: 'Venter', expired: 'Feilet', failed: 'Feilet', notSent: 'Ikke sendt', loading: 'Sjekker', unknown: 'Ukjent' },
      help: { title: 'Hvorfor bekrefte e-posten din?', reason1: 'Sikre kontoen med tofaktor-autentisering', reason2: 'Motta viktige sikkerhetsmeldinger', reason3: 'Aktivere funksjonalitet for tilbakestilling av passord', reason4: 'Følge beste praksis for sikkerhet', needHelp: 'Trenger hjelp?', supportGuide: 'Besøk vår støtteguide', supportUrl: '/støtte/e-postbekreftelse' }
    },
  },

  modal: {
    auth: {
      close: 'Lukk modal',
      opened: 'Autentiseringsmodal åpnet',
      closed: 'Autentiseringsmodal lukket',
      loading: {
        title: 'Autentiserer',
        message: 'Behandler autentiseringsdata...',
      },
      success: {
        login: 'Påloggingen var vellykket!',
        register: 'Registreringen var vellykket!',
        default: 'Autentisering fullført!',
      },
      error: {
        title: 'Autentiseringsfeil',
        message: 'En feil oppstod under autentisering. Vennligst prøv igjen.',
        close: 'Det oppstod en feil ved lukking av modal.',
      },
      mode: {
        signin: {
          title: 'Logg inn',
          description: 'Logg inn på kontoen din for å fortsette',
        },
        signup: {
          title: 'Opprett konto',
          description: 'Opprett en ny konto for å komme i gang',
        },
        forgot: {
          title: 'Tilbakestill passord',
          description: 'Oppgi e-postadressen din for å tilbakestille passordet',
        },
        verify: {
          title: 'Bekreft konto',
          description: 'Bekreft din e-postadresse',
        },
        sso: {
          title: 'Enterprise SSO',
          description: 'Logg inn med organisasjonens legitimasjon',
        },
        organization: {
          title: 'Organisasjonstilgang',
          description: 'Få tilgang til organisasjonsdashbordet',
        },
        default: {
          title: 'Autentisering',
          description: 'Autentiser deg for å fortsette',
        },
      },
    },

    confirmation: {
      opened: 'Bekreftelsesmodal åpnet',
      closed: 'Bekreftelsesmodal lukket',
      cancelled: 'Handlingen ble avbrutt',
      success: {
        action: 'Handlingen ble fullført!',
        deleted: 'Elementet ble slettet',
        confirmed: 'Handlingen ble bekreftet',
        logout: 'Du ble logget ut',
        permission: 'Tillatelser ble oppdatert',
        cookies: 'Cookies ble slettet',
      },
      error: {
        action: 'En feil oppstod under utføring',
        network: 'Nettverksfeil - prøv igjen',
        permission: 'Du har ikke tillatelse til denne handlingen',
      },
    },
  },

  consent: {
    banner: {
      message: 'Ved å klikke «Godta», godtar du lagring av informasjonskapsler på enheten din.',
      messageMobile: 'Ved å klikke «Godta», godtar du lagring av informasjonskapsler.',
      settings: 'Innstillinger',
      reject: 'Avvis',
      accept: 'Godta',
      settingsLabel: 'Åpne personverninnstillinger',
    },

    preferences: {
      title: 'Personvernpreferanser',
      close: 'Lukk',
      intro: 'Når du besøker et nettsted, kan det lagre eller hente informasjon på nettleseren din, for det meste i form av informasjonskapsler. Noe er nødvendig for at siden skal fungere, annet hjelper oss å forbedre opplevelsen din. Denne informasjonen identifiserer deg vanligvis ikke direkte, men kan gi en mer personlig opplevelse.',
      allowAll: 'Tillat',
      rejectAll: 'Avvis alle',
      acceptAll: 'Godta alle',
      saveChoices: 'Lagre valg',
      showDetails: 'Vis detaljer',
      hideDetails: 'Skjul detaljer',
    },

    categories: {
      necessary: {
        title: 'Strengt nødvendige informasjonskapsler',
        description: 'Nødvendig for grunnleggende funksjonalitet og kan ikke slås av i våre systemer.',
      },
      performance: {
        title: 'Ytelseskapsler',
        description: 'Lar oss telle besøk og trafikkilder for å måle og forbedre ytelse. Hjelper oss å vite hvilke sider som er mest og minst populære og hvordan besøkende beveger seg på siden.',
      },
      functional: {
        title: 'Funksjonelle informasjonskapsler',
        description: 'Muliggjør utvidede funksjoner og personalisering. Kan settes av oss eller tredjepartstjenester.',
      },
      marketing: {
        title: 'Målretting/markedsføring',
        description: 'Brukes til å bygge en interesseprofil og vise relevante annonser på andre nettsteder.',
      },
    },

    actions: {
      accepted: 'Samtykke godtatt',
      rejected: 'Samtykke avvist',
      settingsOpened: 'Personverninnstillinger åpnet',
      allAccepted: 'Alle samtykker godtatt',
      allRejected: 'Alle samtykker avvist',
      choicesSaved: 'Samtykkevalg lagret',
    },

    storage: {
      error: 'Kunne ikke lagre samtykkevalg',
      success: 'Samtykkevalg lagret',
    },

    script: {
      loadError: 'Kunne ikke laste skript',
      loadSuccess: 'Skript lastet',
    },

    validation: {
      error: 'Ugyldig samtykkedata',
    },

    reset: {
      success: 'Samtykkeinnstillinger tilbakestilt',
    },
  },

  language: {
    current: 'Gjeldende språk',
    select: 'Velg språk',
    norwegian: 'Norsk',
    english: 'Engelsk',
    changed: 'Språket ble endret',
  },

  social: {
    orWith: 'eller med',
    signInWith: 'Logg inn med {{provider}}',
    unavailable: '{{provider}} (ikke tilgjengelig)',
    providers: {
      google: 'Google',
      microsoft: 'Microsoft',
      okta: 'Okta',
      vipps: 'Vipps',
    },
  },

  passkey: {
    title: 'Passkey',
    register: 'Opprett passkey',
    authenticate: 'Bruk passkey',
    manage: 'Administrer passkeys',
    delete: 'Slett passkey',
    creating: 'Oppretter passkey...',
    authenticating: 'Autentiserer...',
    deleting: 'Sletter...',
    unsupported: 'Passkeys støttes ikke',
    unsupportedMessage: 'Din nettleser støtter ikke passkeys. Vennligst bruk en moderne nettleser eller prøv passord-autentisering.',
    emailRequired: 'Skriv inn e-post for å opprette passkey',
    description: 'Bruk passkey for raskere og tryggere innlogging',
    quickAuth: 'Bruk passkey',
    noPasskeys: 'Ingen passkeys registrert',
    noPasskeysDescription: 'Legg til en passkey for raskere og sikrere autentisering',
    deleteConfirm: 'Er du sikker på at du vil slette denne passkeyen?',
    lastUsed: 'Sist brukt',
    created: 'Opprettet',
    errors: {
      registrationFailed: 'Kunne ikke registrere passkey',
      authenticationFailed: 'Kunne ikke autentisere med passkey',
      deleteFailed: 'Kunne ikke slette passkey',
    },
  },

  callback: {
    title: 'Fullfører innlogging',
    processing: 'Vennligst vent mens vi behandler autentiseringen din.',
    success: 'Innlogging vellykket!',
    redirecting: 'Omdirigerer til dashbordet ditt...',
    error: 'Autentisering feilet. Vennligst prøv igjen.',
    redirectingToSignIn: 'Omdirigerer til innloggingssiden...',
    oauth: {
      error: 'OAuth-autentisering feilet',
      cancelled: 'Autentisering ble avbrutt',
      denied: 'Tilgang ble nektet',
      timeout: 'Autentisering gikk ut på tid',
    },
  },

  // Dashboard translations
  dashboard: {
    aquatiqCard: {
      publishedBy: 'Publisert av',
      readMore: 'Les mer',
      viewAllCollaborators: 'Se alle samarbeidspartnere',
      noUpdates: 'Ingen oppdateringer tilgjengelig',
      categories: {
        announcement: 'Kunngjøring',
        event: 'Arrangement',
        product: 'Produkt',
        business: 'Bedrift',
      },
    },
    weather: {
      title: 'Vær',
      humidity: 'Fuktighet',
      wind: 'Vind',
      pressure: 'Trykk',
      noData: 'Kunne ikke laste værdata',
    },
    news: {
      title: 'Nyheter',
      noNews: 'Ingen nyheter tilgjengelig',
    },
    traffic: {
      title: 'Trafikk',
      noData: 'Ingen trafikkdata tilgjengelig',
    },
  },
};
