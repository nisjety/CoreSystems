# Authentication System Architecture

This directory contains a comprehensive, modular authentication system built around the existing `auth-page.tsx` component. The structure has been refactored to provide better organization, reusability, and maintainability.

## 📁 Directory Structure

```
src/
├── types/                     # TypeScript type definitions
│   ├── auth.ts               # Core auth types (AuthMode, AuthFormData, etc.)
│   ├── verification.ts       # 2FA & verification types
│   └── index.ts              # Type exports
│
├── utils/                    # Utility functions
│   ├── consent.ts           # GDPR consent management
│   ├── validation.ts        # Form validation utilities
│   ├── oauth.ts             # OAuth URL construction
│   ├── cookies.ts           # Cookie management
│   ├── error-handling.ts    # Error utilities
│   ├── url.ts               # URL helpers
│   ├── device.ts            # Device detection
│   ├── feature-detection.ts # Feature detection (passkeys, etc.)
│   ├── verification.ts      # Verification utilities
│   └── index.ts             # Utility exports
│
├── config/                  # Configuration
│   ├── auth.ts             # Auth configuration constants
│   └── index.ts            # Config exports
│
├── constants/              # Application constants
│   ├── auth.ts            # Auth-related constants
│   └── index.ts           # Constant exports
│
├── hooks/                 # React hooks (already existing)
│   ├── use-auth.ts        # Main auth hook
│   ├── use-consent.ts     # Consent management hook
│   ├── use-passkey.ts     # Passkey functionality
│   └── ...               # Other existing hooks
│
├── components/
│   ├── auth/
│   │   ├── core/                    # Core auth components
│   │   │   ├── AuthPage.tsx         # Main auth container (moved from root)
│   │   │   ├── AuthTabs.tsx         # Tab navigation component
│   │   │   └── index.ts             # Core exports
│   │   │
│   │   ├── providers/               # Authentication providers
│   │   │   ├── SocialProviders.tsx  # OAuth provider buttons
│   │   │   ├── PasskeyButtons.tsx   # Passkey authentication
│   │   │   └── index.ts             # Provider exports
│   │   │
│   │   ├── verification/            # 2FA & Verification components
│   │   │   ├── SignInWithVerification.tsx    # Enhanced sign-in flow
│   │   │   ├── TotpQrDemo.tsx               # TOTP setup demo
│   │   │   ├── SecuritySettings.tsx         # Security management
│   │   │   ├── TwoFactorSetup.tsx           # 2FA setup wizard
│   │   │   └── index.ts                     # Verification exports
│   │   │
│   │   ├── consent/                 # GDPR consent components
│   │   │   ├── ConsentBanner.tsx    # Privacy banner
│   │   │   └── index.ts             # Consent exports
│   │   │
│   │   └── index.ts                 # Main auth exports
│   │
│   └── index.ts                     # All component exports
│
├── examples/                        # Usage examples
│   ├── BasicAuthPage.tsx           # Basic usage
│   ├── CustomAuthFlow.tsx          # Custom implementation
│   ├── MinimalVerificationSetup.tsx # Simple 2FA setup
│   └── ...                         # Additional examples
│
└── stories/ (optional)             # Storybook stories
    ├── AuthForms.stories.tsx
    ├── SocialProviders.stories.tsx
    └── ...
```

## 🎯 Complete Authentication System

### AuthPage - Unified Solution

Our new `AuthPage` component is a comprehensive, production-ready authentication interface that brings together all 27 modular components:

**🔐 Authentication Methods:**
- Email/Password with validation
- Social OAuth (Google, Microsoft)
- Passkey/WebAuthn authentication
- Enterprise SSO with domain discovery

**🔒 Security Features:**
- Multi-factor authentication (Email OTP, SMS OTP, TOTP)
- Multi-channel verification (Email, SMS, Phone, WhatsApp)
- Security settings and device management
- Role-based access control

**🏢 Enterprise Features:**
- Organization management and invitations
- Enterprise SSO integration
- Advanced admin controls
- Audit logging and security monitoring

**♿ Accessibility & Compliance:**
- WCAG 2.1 AA compliance
- GDPR consent management
- Internationalization ready
- High contrast and screen reader support

## 🔧 Key Components

### Core Components

- **AuthPage**: The main authentication container (equivalent to the original `auth-page.tsx`)
- **AuthTabs**: Tab navigation for different auth modes (signin, signup, SSO, etc.)

### Provider Components

- **SocialProviders**: OAuth provider buttons (Google, Microsoft, etc.)
- **PasskeyButtons**: Passkey authentication UI

### Verification Components

- **SignInWithVerification**: Enhanced sign-in with 2FA support
- **TwoFactorSetup**: Complete 2FA setup wizard
- **SecuritySettings**: Security management dashboard
- **TotpQrDemo**: TOTP QR code setup

### Consent Components

- **ConsentBanner**: GDPR-compliant cookie consent banner

## 🚀 Usage Examples

### Basic Usage (Drop-in Replacement)

```tsx
import { AuthPage } from './components/auth';

export function App() {
  return (
    <div className="min-h-screen flex items-center justify-center">
      <AuthPage redirectTo="/dashboard" />
    </div>
  );
}
```

### Complete Feature Set (Recommended)

```tsx
import { AuthPage } from './components/auth';

export function App() {
  return (
    <AuthPage
      initialMode="signin"
      features={{
        socialAuth: true,
        passkeys: true,
        enterpriseSSO: true,
        organizationManagement: true,
        twoFactor: true,
        multiChannelVerification: true,
        gdprConsent: true,
        accessibilityMode: true
      }}
      customization={{
        companyName: 'Your Company',
        supportEmail: 'support@yourcompany.com'
      }}
      onAuthSuccess={(user) => console.log('Welcome!', user)}
      onAuthError={(error) => console.error('Auth error:', error)}
    />
  );
}
```

### Enterprise Authentication

```tsx
import { AdminAuthPage } from './components/auth';

export function AdminPanel() {
  return <AdminAuthPage />;
}
```

### Custom Authentication Flow

```tsx
import { AuthTabs, SocialProviders } from './components/auth';
import { useAuth } from './hooks/use-auth';

export function CustomAuth() {
  const [mode, setMode] = useState('signin');
  const { signIn } = useAuth();

  return (
    <div className="auth-container">
      <AuthTabs currentMode={mode} onModeChange={setMode} />
      <SocialProviders onProviderClick={handleOAuth} />
      {/* Your custom forms here */}
    </div>
  );
}
```

### Protected Routes

```tsx
import { ProtectedAuthPage } from './components/auth';

export function LoginPage() {
  // Only shows auth form if user is not authenticated
  return <ProtectedAuthPage />;
}
```

## 🔄 Migration Guide

### From Old Structure

The original `auth-page.tsx` has been moved to `components/auth/core/AuthPage.tsx` and broken down into smaller, reusable components.

**Before:**
```tsx
import { AuthPage } from './components/auth';
```

**After:**
```tsx
import { AuthPage } from './components/auth';
// or for specific components:
import { AuthPage } from './components/auth/core';
```

### Imports

All components are re-exported from the main auth index, so you can import everything from:

```tsx
import { 
  AuthPage, 
  AuthTabs, 
  SocialProviders, 
  ConsentBanner,
  TwoFactorSetup 
} from './components/auth';
```

## 🎯 Benefits of This Structure

1. **Modularity**: Components can be used independently
2. **Reusability**: Share components across different parts of your app
3. **Maintainability**: Clear separation of concerns
4. **Testing**: Easier to test individual components
5. **Customization**: Build custom auth flows with existing components
6. **Backward Compatibility**: Original auth-page functionality preserved

## 🧪 Testing

Each component directory includes its corresponding test utilities:

```
test-utils/
├── auth-test-utils.tsx        # Auth testing utilities
├── verification-test-utils.tsx # Verification testing
└── mocks.ts                   # Mock data
```

## 📖 Documentation

- See `examples/` for comprehensive usage examples
- Check individual component files for detailed prop documentation
- All components include TypeScript interfaces for better developer experience

## 🔐 Security Considerations

- All components follow security best practices
- CSRF protection through Better Auth integration
- Secure cookie handling
- Input validation and sanitization
- GDPR compliance through consent management

This structure provides a solid foundation for building authentication features while maintaining the full functionality of the original auth-page component.
