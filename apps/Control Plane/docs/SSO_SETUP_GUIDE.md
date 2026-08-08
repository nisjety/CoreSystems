# 🔐 SSO Setup Guide: Enterprise Single Sign-On

## Overview

This guide walks you through setting up Enterprise Single Sign-On (SSO) with automatic organization provisioning using Better Auth's SSO plugin. Our implementation supports **Microsoft Entra ID (Azure AD)** and **Google Workspace** with advanced organization provisioning features.

## 📋 Prerequisites

- ✅ Better Auth configured with organization plugin
- ✅ Docker environment with SSO environment variables
- ✅ Admin access to Microsoft Entra ID or Google Workspace
- ✅ Valid domain ownership for SSO provider

## 🏗️ Architecture Overview

```
SSO Login Flow:
┌─────────────────┐    ┌─────────────────┐    ┌─────────────────┐
│   SSO Provider  │───▶│   Auth Service  │───▶│  Organization   │
│ (Entra/Google)  │    │  Better Auth    │    │   Auto-Create   │
└─────────────────┘    └─────────────────┘    └─────────────────┘
                                  │
                                  ▼
                       ┌─────────────────┐
                       │   Role-Based    │
                       │   Assignment    │
                       │   (Admin/Member)│
                       └─────────────────┘
```

## 🎯 Features Supported

### ✅ Organization Provisioning
- **Auto-creation** from SSO domain attributes
- **Domain-based** organization matching
- **NATS event publishing** for org lifecycle

### ✅ Role-Based Assignment
- **Dynamic admin assignment** based on:
  - Job titles: `manager`, `director`, `admin`
  - Department: `it` department gets admin access
- **Default role**: `member` for all other users

### ✅ Provider Support  
- **Microsoft Entra ID**: Full Azure AD integration
- **Google Workspace**: Complete G Suite support
- **OIDC Generic**: Any OIDC-compliant provider

---

## 🔧 Provider Setup

### 1️⃣ Microsoft Entra ID (Azure AD) Setup

#### Step 1: Create App Registration

1. **Access Azure Portal**
   ```
   https://portal.azure.com
   Navigate to: Azure Active Directory → App registrations → New registration
   ```

2. **Configure Application**
   ```
   Name: AquaTiq SSO
   Account types: Single tenant (recommended) or Multitenant
   Redirect URI: https://your-domain.com/api/auth/callback/microsoft
   ```

3. **Collect Application Details**
   ```bash
   # From "Overview" page
   Application (client) ID: 12345678-1234-1234-1234-123456789abc
   Directory (tenant) ID: 87654321-4321-4321-4321-987654321def
   ```

#### Step 2: Generate Client Secret

1. **Create Secret**
   ```
   Navigate to: Certificates & secrets → Client secrets → New client secret
   Description: AquaTiq SSO Secret
   Expires: 24 months (recommended)
   ```

2. **Copy Secret Value**
   ```bash
   # ⚠️ Copy immediately - won't be shown again
   Client Secret: abcdef123456-secretvalue-789xyz
   ```

#### Step 3: Configure API Permissions

1. **Add Permissions**
   ```
   Navigate to: API permissions → Add a permission → Microsoft Graph
   
   Required permissions:
   ✅ User.Read (Delegated) - Basic profile
   ✅ Directory.Read.All (Application) - Organization info
   ✅ Organization.Read.All (Application) - Company details
   ```

2. **Grant Admin Consent**
   ```
   Click "Grant admin consent for [Your Organization]"
   ✅ All permissions should show "Granted"
   ```

#### Step 4: Optional Claims Configuration

1. **Add Optional Claims**
   ```
   Navigate to: Token configuration → Add optional claim
   Token type: ID token
   
   Recommended claims:
   ✅ email
   ✅ family_name  
   ✅ given_name
   ✅ job_title
   ✅ department
   ✅ company
   ```

### 2️⃣ Google Workspace Setup

#### Step 1: Create OAuth Application

1. **Access Google Cloud Console**
   ```
   https://console.cloud.google.com
   Navigate to: APIs & Services → Credentials → Create Credentials → OAuth client ID
   ```

2. **Configure OAuth Consent Screen**
   ```
   User Type: Internal (for workspace) or External
   App name: AquaTiq SSO
   User support email: admin@your-domain.com
   Developer contact: admin@your-domain.com
   ```

3. **Create OAuth Client**
   ```
   Application type: Web application
   Name: AquaTiq SSO Client
   Authorized redirect URIs: https://your-domain.com/api/auth/callback/google
   ```

#### Step 2: Configure Domain Verification

1. **Verify Domain Ownership**
   ```
   Navigate to: Google Admin → Security → API controls → Domain verification
   Add your domain: your-domain.com
   Complete verification process
   ```

#### Step 3: Enable APIs

1. **Required APIs**
   ```
   Navigate to: APIs & Services → Library
   
   Enable these APIs:
   ✅ Google+ API (for profile info)
   ✅ Admin SDK API (for organization data)  
   ✅ Directory API (for user details)
   ```

---

## ⚙️ Environment Configuration

### Update Docker Compose

Add these variables to your `.env` file or docker-compose.yml:

```bash
# SSO Global Configuration
SSO_ENABLED=true
SSO_ORG_PROVISIONING_DISABLED=false
SSO_DEFAULT_ROLE=member

# Microsoft Entra ID Configuration
MICROSOFT_CLIENT_ID=12345678-1234-1234-1234-123456789abc
MICROSOFT_CLIENT_SECRET=abcdef123456-secretvalue-789xyz
MICROSOFT_TENANT_ID=87654321-4321-4321-4321-987654321def
MICROSOFT_SCOPE=openid profile email User.Read Directory.Read.All

# Google Workspace Configuration  
GOOGLE_CLIENT_ID=123456789012-abcdefghijklmnopqrstuvwxyz012345.apps.googleusercontent.com
GOOGLE_CLIENT_SECRET=GOCSPX-1234567890abcdefghijklmnop

# OIDC Generic Provider (Optional)
OIDC_PROVIDER_ENABLED=false
OIDC_LOGIN_PAGE=/sign-in
OIDC_CONSENT_PAGE=/consent
OIDC_ALLOW_DYNAMIC_REGISTRATION=false
```

### Environment Variable Reference

| Variable | Required | Description | Example |
|----------|----------|-------------|---------|
| `SSO_ENABLED` | Yes | Enable/disable SSO | `true` |
| `SSO_ORG_PROVISIONING_DISABLED` | No | Disable auto org creation | `false` |
| `SSO_DEFAULT_ROLE` | No | Default user role | `member` |
| `MICROSOFT_CLIENT_ID` | Yes* | Azure app client ID | `12345678-1234-...` |
| `MICROSOFT_CLIENT_SECRET` | Yes* | Azure app secret | `abcdef123456-...` |
| `MICROSOFT_TENANT_ID` | Yes* | Azure tenant ID | `87654321-4321-...` |
| `GOOGLE_CLIENT_ID` | Yes** | Google OAuth client ID | `123456789012-abc...` |
| `GOOGLE_CLIENT_SECRET` | Yes** | Google OAuth secret | `GOCSPX-1234567...` |

*Required for Microsoft Entra ID SSO  
**Required for Google Workspace SSO

---

## 🚀 Testing & Validation

### 1️⃣ Test SSO Authentication

```bash
# Test Microsoft Entra ID login
curl -X POST http://localhost:3001/api/auth/sign-in/microsoft \
  -H "Content-Type: application/json" \
  -d '{"redirectTo": "/dashboard"}'

# Test Google Workspace login  
curl -X POST http://localhost:3001/api/auth/sign-in/google \
  -H "Content-Type: application/json" \
  -d '{"redirectTo": "/dashboard"}'
```

### 2️⃣ Verify Organization Provisioning

```javascript
// Check if organization was auto-created
const response = await fetch('/api/organizations/my', {
  headers: { 'Authorization': 'Bearer ' + authToken }
});

const organizations = await response.json();
console.log('Auto-created organizations:', organizations);
```

### 3️⃣ Validate Role Assignment

```javascript
// Check user role in organization
const roleResponse = await fetch('/api/organizations/{orgId}/members/me', {
  headers: { 'Authorization': 'Bearer ' + authToken }
});

const membership = await roleResponse.json();
console.log('User role:', membership.role); // Should be 'admin' or 'member'
```

### 4️⃣ Monitor NATS Events

```bash
# Subscribe to organization creation events
docker exec -it backend-coresystem-nats-local-1 nats sub "organization.created"
```

Expected output:
```json
{
  "id": "org_12345",
  "name": "Acme Corporation",
  "domain": "acme.com",
  "createdBy": "user_67890",
  "createdAt": "2024-02-08T10:30:00Z",
  "source": "sso_provisioning"
}
```

---

## 🔍 Troubleshooting

### Common Issues

#### ❌ "Invalid redirect URI"
```bash
# Problem:
Error: The redirect URI 'https://localhost:3001/...' is not registered

# Solution:
1. Update provider redirect URIs to match your domain
2. For development: https://localhost:3001/api/auth/callback/microsoft
3. For production: https://tools.coresystem.com/api/auth/callback/microsoft
```

#### ❌ "Organization not created"
```bash
# Problem:
SSO login successful but no organization created

# Debug:
1. Check SSO_ORG_PROVISIONING_DISABLED=false
2. Verify domain info in SSO response
3. Check NATS event publishing
4. Review application logs for errors

# Logs to check:
docker logs backend-auth-service-1 | grep "🏢\|organization"
```

#### ❌ "Role not assigned correctly"
```bash
# Problem:
User always gets 'member' role instead of 'admin'

# Debug:
1. Verify job title/department in SSO claims
2. Check role assignment logic in auth.ts
3. Ensure optional claims are configured
4. Test role logic with console.log

# Expected attributes:
job_title: "Manager", "Director", "Admin"
department: "IT", "Information Technology"
```

### Debug Mode

Enable detailed SSO logging:

```bash
# Add to environment
DEBUG_SSO=true
LOG_LEVEL=debug

# View detailed logs
docker logs -f backend-auth-service-1 | grep -i sso
```

---

## 🎯 Advanced Configuration

### Custom Role Assignment Logic

Modify role assignment in [auth.ts](auth/src/auth/auth.ts#L720-L740):

```typescript
getRole: async ({ userInfo }) => {
  const department = userInfo.attributes?.department;
  const jobTitle = userInfo.attributes?.jobTitle;
  const email = userInfo.email;

  // C-level executives
  if (jobTitle?.toLowerCase().includes('ceo') || 
      jobTitle?.toLowerCase().includes('cto')) {
    return 'admin';
  }

  // Domain-based admin assignment
  if (email.endsWith('@admin.company.com')) {
    return 'admin';
  }

  // Department-based assignment
  if (department?.toLowerCase().includes('engineering') ||
      department?.toLowerCase().includes('security')) {
    return 'admin';
  }

  return 'member';
}
```

### Custom Organization Provisioning

```typescript
organizationProvisioning: {
  disabled: false,
  defaultRole: 'member',
  getOrganizationInfo: async ({ userInfo }) => {
    // Extract org info from SSO attributes
    return {
      name: userInfo.attributes?.company || 'Default Organization',
      domain: userInfo.email.split('@')[1],
      metadata: {
        source: 'sso_provisioning',
        provider: userInfo.provider,
        department: userInfo.attributes?.department
      }
    };
  }
}
```

---

## 📊 Monitoring & Analytics

### Key Metrics to Track

```bash
# SSO login success rate
ratio(sso_login_success, sso_login_attempts) * 100

# Organization auto-creation rate  
ratio(org_created_via_sso, sso_login_success) * 100

# Admin role assignment accuracy
ratio(admin_roles_assigned_correctly, admin_roles_assigned) * 100
```

### Health Check Endpoints

```bash
# SSO provider health
GET /api/auth/health/sso
{
  "microsoft": "healthy",
  "google": "healthy", 
  "provisioning": "enabled"
}

# Organization provisioning health
GET /api/organizations/health
{
  "auto_creation": "enabled",
  "nats_events": "connected",
  "role_assignment": "active"
}
```

---

## 🔒 Security Best Practices

### 1️⃣ Provider Security

- ✅ Use **least-privilege** API permissions
- ✅ Enable **admin consent** for organization access
- ✅ Configure **trusted domains** only
- ✅ Rotate **client secrets** every 12 months

### 2️⃣ Application Security

- ✅ Store secrets in **encrypted environment** 
- ✅ Use **HTTPS only** for all redirects
- ✅ Implement **CSRF protection**
- ✅ Enable **audit logging** for SSO events

### 3️⃣ Organization Security

- ✅ Validate **domain ownership** before auto-creation
- ✅ Implement **role escalation** approval workflows
- ✅ Monitor **unusual login patterns**
- ✅ Regular **access reviews** for admin assignments

---

## 📈 Next Steps

After completing SSO setup:

1. **✅ Test End-to-End Flow**
   - Microsoft Entra ID login → Org creation → Role assignment
   - Google Workspace login → Org creation → Role assignment

2. **✅ Update Implementation Plan**
   - Mark Phase 4 as completed in [Implementation-Plan.md](../Implementation-Plan.md)
   - Document SSO configuration decisions

3. **✅ Proceed to Phase 5**
   - Begin subscription tier implementation
   - Configure feature gating for SSO by tier

4. **✅ Production Deployment**
   - Update production environment variables
   - Configure production domains in providers
   - Enable monitoring and alerting

---

## 💡 Support & Resources

### Documentation
- [Better Auth SSO Plugin](https://better-auth.com/docs/plugins/sso)
- [Microsoft Entra ID OAuth](https://docs.microsoft.com/en-us/azure/active-directory/develop/)
- [Google Workspace OAuth](https://developers.google.com/workspace/guides/auth-overview)

### Quick Reference
- **Auth Service**: `http://localhost:3001`
- **SSO Endpoints**: `/api/auth/sign-in/{provider}`
- **Organization API**: `/api/organizations/*`
- **Health Checks**: `/api/*/health`

### Team Contact
- **Lead Developer**: Integration Team
- **SSO Issues**: Report to #auth-service channel
- **Provider Config**: Contact IT Security team

---

**🎉 Congratulations!** Your enterprise SSO is now configured with automatic organization provisioning and role-based access control.