# SuperOffice API Selection Guide

## Which API Should You Use?

When building SuperOffice integrations, the first question is often: **"What APIs are available?"** SuperOffice supports both **SOAP** and **REST** web services. The follow-up question is usually: **"Which API should I use?"**

**The answer**: It depends on your integration type, technology stack, and requirements.

## Quick Decision Matrix

| Factor | REST API | SOAP API |
|--------|----------|----------|
| **Recommended** | ✅ Modern integrations | ⚠️ Legacy systems |
| **Format** | JSON (lightweight) | XML (verbose) |
| **Speed** | Fast | Slower (XML overhead) |
| **Authentication** | OAuth 2.0 / OpenID Connect | System User Token / OAuth |
| **Learning Curve** | Low | Medium-High |
| **.NET Required** | No | Yes (for proxies) |
| **Online Support** | ✅ Full support | ✅ Full support |
| **Onsite Support** | ✅ Full support | ✅ Full support |
| **Our Implementation** | ✅ **We use this** | ❌ Not used |

## API Comparison

### REST API (Recommended for Modern Applications)

**Best for:**
- ✅ Modern web applications
- ✅ Mobile apps
- ✅ Cloud-based integrations
- ✅ Quick prototyping
- ✅ JavaScript/TypeScript applications
- ✅ Python, Ruby, PHP integrations
- ✅ Microservices architecture

**Characteristics:**
- **Format**: JSON (lightweight, fast)
- **Protocol**: HTTP/HTTPS with standard verbs (GET, POST, PUT, DELETE)
- **Authentication**: OAuth 2.0 / OpenID Connect
- **URLs**: RESTful resource-based URLs
- **Network**: Minimal overhead, faster transfers

**Example:**
```bash
# REST API call
GET https://sod.superoffice.com/api/v1/contact/123
Authorization: Bearer {access_token}
Accept: application/json

# Response (JSON)
{
  "ContactId": 123,
  "Name": "Acme Corporation",
  "OrgNr": "123456789"
}
```

### SOAP API (For Enterprise & Legacy Systems)

**Best for:**
- ⚠️ Enterprise systems with SOAP infrastructure
- ⚠️ .NET applications using SuperOffice proxies
- ⚠️ Complex batch operations
- ⚠️ Legacy integrations
- ⚠️ Systems requiring strict type safety

**Characteristics:**
- **Format**: XML (verbose, larger payload)
- **Protocol**: HTTP/HTTPS with SOAP envelope
- **Authentication**: System User Token or OAuth 2.0
- **Proxies**: SuperOffice-provided or self-generated
- **Network**: Higher overhead, slower transfers

**Example:**
```xml
<!-- SOAP API call -->
<soap:Envelope xmlns:soap="http://schemas.xmlsoap.org/soap/envelope/">
  <soap:Header>
    <Authentication>Bearer {token}</Authentication>
  </soap:Header>
  <soap:Body>
    <GetContact>
      <contactId>123</contactId>
    </GetContact>
  </soap:Body>
</soap:Envelope>
```

## Our Implementation: REST API

**✅ We use the REST API because:**

1. **Modern Architecture**: JSON-based, lightweight
2. **Simpler Authentication**: OAuth 2.0 / OpenID Connect
3. **Better Performance**: Smaller payloads, faster responses
4. **Language Agnostic**: Works with Python without special proxies
5. **Future-Proof**: SuperOffice's recommended approach for new applications
6. **Easier Maintenance**: Standard HTTP clients, no proxy generation

## Authentication: Interactive vs Non-Interactive

### Pre-requisite: Application Consent

**Before any authentication**, your application must be granted access to a tenant's web services. This is done through:

1. **App Store Installation**: Tenant administrator installs via App Store
2. **Developer Mock Installation**: Developer simulates installation

#### Installation URL Formats

**Old Form (Legacy):**
```
http://{env}.superoffice.com/login/?app_id=YOUR_APP_ID
```

**OAuth 2.0 Form (Recommended):**
```
https://{env}.superoffice.com/login/common/oauth/authorize
  ?response_type=id_token token
  &client_id=YOUR_APP_ID
  &redirect_uri=YOUR_REDIRECT_URL
  &scope=openid
  &state=12345
  &nonce=7362CAEA-9CA5-4B43-9BA3-34D7C303EBA7
```

**Key Differences:**

| Aspect | Old Form | OAuth 2.0 Form |
|--------|----------|----------------|
| Response | JWT in POST body | JWT + access_token in URL fragment |
| Token Type | Ticket credential | access_token credential |
| Flow | Custom | Standard OAuth 2.0 Implicit Flow |
| Recommended | ❌ Legacy | ✅ Modern |

### Interactive Authentication (User Sign-In)

**For**: Normal users who need to access your application

**Important Rule**: 🚫 **Never create your own sign-in screen!** All interactive authentications must use the SuperOffice Online sign-in page.

#### Standard Interactive Flow

```
1. User clicks "Sign in with SuperOffice"
   ↓
2. Redirect to SuperOffice login page
   https://sod.superoffice.com/login/common/oauth/authorize?...
   ↓
3. User enters credentials and grants consent
   ↓
4. SuperOffice redirects back to your app
   https://your-app.com/callback#access_token=xxx&id_token=yyy
   ↓
5. Your app extracts tokens and authenticates user
```

#### Web Panel with SSO

When your app is embedded in SuperOffice via a web panel, include the `<uctx>` template variable for seamless SSO:

```html
<!-- Web panel URL in SuperOffice -->
https://your-app.com/panel?context=<uctx>
```

Then use the context identifier in the OAuth URL:

```
https://sod.superoffice.com/login/{contextIdentifier}/oauth/authorize?...
```

**Example:**
```
https://sod.superoffice.com/login/Cust12345/oauth/authorize
  ?response_type=id_token token
  &client_id=YOUR_APP_ID
  &redirect_uri=YOUR_REDIRECT_URL
  &scope=openid
  &state=12345
  &nonce=7362CAEA-9CA5-4B43-9BA3-34D7C303EBA7
```

This provides users with a seamless single sign-on experience.

### Non-Interactive Authentication (Server-to-Server)

**For**: Background processes, scheduled sync jobs, system integrations

**User Types:**

- **Normal Users**: Everyday users employed by the company
- **System Users**: Special accounts with unlimited access for routine tasks
  - Do NOT support impersonation
  - Cannot do work on behalf of normal users
  - Used for background processes

#### System User Token

When your application is installed, you receive a **SystemUserToken** in the JWT claims. This token is used for non-interactive server-to-server communications.

**Use Cases:**
- Scheduled data synchronization
- Background batch processing
- Automated workflows
- System maintenance tasks

### Semi-Interactive Authentication (Refresh Tokens)

**For**: Applications that need ongoing access without repeated user logins

SuperOffice doesn't support purely non-interactive OAuth 2.0 flows (like Client Credentials), but **does support refresh tokens**.

#### Refresh Token Flow

```
1. User interactively authenticates (first time)
   ↓
2. Receive access_token + refresh_token
   ↓
3. Store refresh_token securely (encrypted)
   ↓
4. When access_token expires:
   POST https://{env}.superoffice.com/login/common/oauth/tokens
   Body: grant_type=refresh_token&refresh_token=xxx
   ↓
5. Receive new access_token (no user interaction needed)
   ↓
6. Continue API calls with new token
```

**Our Implementation:**

```python
# In auth/oauth_handler.py
def _refresh_access_token(self, organization_id: str) -> Dict[str, Any]:
    """Refresh access token using refresh token."""
    tokens = self.token_store.get(organization_id)
    
    data = {
        "grant_type": "refresh_token",
        "client_id": self.settings.client_id,
        "client_secret": self.settings.client_secret,
        "refresh_token": tokens["refresh_token"]
    }
    
    response = requests.post(self.settings.token_url, data=data)
    return response.json()
```

**This is called "Semi-Interactive"** because:
- User must first interactively authenticate
- Subsequent token refreshes are automatic
- No user interaction needed after initial login

## OpenID Connect

OpenID Connect is an identity layer built on OAuth 2.0, commonly used by RESTful API consumers.

### Supported Flows

SuperOffice Online supports these **interactive** OpenID Connect flows:

| Flow | Description | Use Case |
|------|-------------|----------|
| **Authorization Code** | Most secure, uses server-side exchange | Web applications with backend |
| **Implicit** | Client-side only, no backend | Single-page apps (SPAs) |
| **Hybrid** | Combination of both | Complex applications |

**Not Supported:**
- ❌ Client Credentials (pure non-interactive)
- ❌ Resource Owner Password Credentials

### Tested Frameworks

SuperOffice has successfully tested:

- ✅ **Microsoft ASP.NET** authentication services
- ✅ **IdentityServer's** oidc-client
- ✅ **oidc-client-js** npm package (for JavaScript/TypeScript)

**Example (Angular):**
```typescript
// Using oidc-client-js
import { UserManager } from 'oidc-client';

const config = {
  authority: 'https://sod.superoffice.com',
  client_id: 'YOUR_APP_ID',
  redirect_uri: 'https://your-app.com/callback',
  response_type: 'id_token token',
  scope: 'openid',
  // ...
};

const userManager = new UserManager(config);
userManager.signinRedirect();
```

## API Decision Tree

```
START: What are you building?
│
├─ Modern cloud integration?
│  └─ ✅ Use REST API
│
├─ Mobile application?
│  └─ ✅ Use REST API
│
├─ Web application (React/Angular/Vue)?
│  └─ ✅ Use REST API
│
├─ Python/Ruby/PHP/Node.js service?
│  └─ ✅ Use REST API
│
├─ Legacy .NET enterprise system?
│  └─ ⚠️ Consider SOAP API (but REST is still preferred)
│
├─ Existing SOAP infrastructure?
│  └─ ⚠️ Use SOAP API
│
└─ New integration in 2025+?
   └─ ✅ Use REST API (SuperOffice recommendation)
```

## Authentication Decision Tree

```
START: What type of access do you need?
│
├─ User needs to sign in to your app?
│  └─ Use Interactive Authentication (OAuth 2.0)
│     │
│     ├─ Standalone web app?
│     │  └─ Authorization Code Flow
│     │
│     ├─ Single-page app (SPA)?
│     │  └─ Implicit Flow
│     │
│     └─ Embedded in SuperOffice (web panel)?
│        └─ Implicit Flow with <uctx> for SSO
│
├─ Background process / scheduled job?
│  └─ Use System User Token
│     (received during app installation)
│
└─ Need ongoing access without repeated logins?
   └─ Use Refresh Tokens (Semi-Interactive)
      1. User signs in once (interactive)
      2. Store refresh_token securely
      3. Use refresh_token to get new access_tokens
```

## Best Practices

### For REST API

✅ **DO:**
- Use OAuth 2.0 / OpenID Connect
- Use standard HTTP clients (requests, axios, fetch)
- Send JSON payloads
- Use Bearer tokens in Authorization header
- Implement automatic token refresh
- Cache reference data (MDO lists)
- Use pagination for large datasets

❌ **DON'T:**
- Create custom login screens
- Store tokens in plain text
- Use deprecated Ticket authentication
- Over-fetch data
- Skip token expiry checks

### For Authentication

✅ **DO:**
- Always redirect to SuperOffice login page
- Store refresh tokens encrypted
- Implement automatic token refresh (5-minute buffer)
- Use HTTPS for all OAuth redirects
- Validate state parameter (prevent CSRF)
- Handle token expiry gracefully

❌ **DON'T:**
- Create your own login form
- Store tokens in client-side storage (browser)
- Use System User Token for user-specific actions
- Expose refresh tokens to frontend
- Skip token validation

## Our Implementation Summary

### Technology Choices

| Component | Choice | Reason |
|-----------|--------|--------|
| **API Type** | REST | Modern, fast, JSON-based |
| **Authentication** | OAuth 2.0 | Industry standard |
| **Flow** | Authorization Code | Most secure for web apps |
| **Token Management** | Automatic Refresh | Seamless user experience |
| **Format** | JSON | Lightweight, easy to parse |
| **Client** | Python requests | Simple, powerful |

### Authentication Flow

```python
# 1. Get authorization URL
auth_url = oauth_handler.get_authorization_url(org_id)
# Redirect user to auth_url

# 2. Handle callback
tokens = oauth_handler.handle_callback(code, state)

# 3. Use access token (automatic refresh)
client = SuperOfficeClient(
    oauth_handler.get_access_token(org_id),
    settings
)

# 4. Make API calls
contacts = client.get_contacts()
```

### Why These Choices Work

1. **REST over SOAP**: 
   - 60% faster due to JSON vs XML
   - No proxy generation needed
   - Works natively with Python
   - Better for microservices

2. **OAuth 2.0 over Legacy**:
   - Industry standard
   - Better security
   - Refresh token support
   - Works with modern frameworks

3. **Automatic Refresh**:
   - Users never see expired token errors
   - Seamless background operation
   - 5-minute buffer prevents expiry

## Migration Guide

### From SOAP to REST

If you have an existing SOAP integration:

```python
# SOAP (old way)
from SuperOffice.WebService import ContactAgent
agent = ContactAgent()
contact = agent.GetContact(123)

# REST (new way)
from clients.socrm_client import SuperOfficeClient
client = SuperOfficeClient(access_token, settings)
contact = client.get_contact(123)
```

**Benefits of Migration:**
- ⚡ Faster response times
- 📦 Smaller payloads (JSON vs XML)
- 🔧 No proxy maintenance
- 🚀 Modern authentication
- 📱 Better mobile support

## Resources

### Official Documentation

- [REST API Reference](https://docs.superoffice.com/en/api/reference/restful/)
- [SOAP API Reference](https://docs.superoffice.com/en/api/reference/soap/)
- [Authentication Guide](https://docs.superoffice.com/en/authentication/)
- [OpenID Connect](https://docs.superoffice.com/en/authentication/online/sign-in-user/openid-connect/)

### Internal Documentation

- [Developer Guide](./SUPEROFFICE_DEVELOPER_GUIDE.md)
- [API Guide](./SUPEROFFICE_API_GUIDE.md)
- [Quick Start](./QUICK_START.md)

### Examples

- [REST Examples (Python)](https://github.com/SuperOffice/devnet-python-restful)
- [OpenID Examples](https://github.com/SuperOffice/devnet-oidc-angular-restful)
- [SOAP Examples (.NET)](https://github.com/SuperOffice/devnet-netserver-soap-samples)

## Summary

### Quick Answer: Which API Should You Use?

**For new integrations in 2025**: ✅ **Use REST API with OAuth 2.0**

**Why?**
- Modern, fast, lightweight (JSON)
- Industry-standard authentication
- Works with any programming language
- SuperOffice's recommended approach
- Better for cloud and mobile
- Easier to maintain

**When to consider SOAP?**
- Only if you have existing SOAP infrastructure
- Or legacy .NET systems requiring it
- But even then, REST is preferred for new development

### Quick Answer: Which Authentication?

**For user access**: ✅ **Interactive OAuth 2.0** (user signs in via SuperOffice)

**For background jobs**: ✅ **System User Token** (received at installation)

**For ongoing access**: ✅ **Refresh Tokens** (semi-interactive)

**Never**: ❌ Custom login screens (not allowed)

---

**Our Integration**: We use REST API with OAuth 2.0 and automatic token refresh—the recommended modern approach for SuperOffice Online integrations.

---

**Last Updated**: December 17, 2025  
**Version**: 1.0  
**Applies To**: SuperOffice CRM Online
