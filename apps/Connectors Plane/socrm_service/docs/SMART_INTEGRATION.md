# SuperOffice Service - Smart Integration Architecture

## Overview

This service provides a **smart integration layer** between your existing Microsoft-authenticated frontend and SuperOffice CRM with automatic currency conversion.

## Key Innovation: Reuse Existing Microsoft Auth

```
┌──────────────────────────────────────────────────────────┐
│                   Your Frontend                          │
│                (Already has Microsoft Auth)              │
│                                                          │
│  ✓ User logs in with Microsoft                          │
│  ✓ Gets access token                                    │
│  ✓ Token works for BOTH:                                │
│    - Your existing frontend APIs                        │
│    - SuperOffice service (no separate login!)           │
└──────────────────────────────────────────────────────────┘
                           │
                           │ Same Microsoft Token
                           ↓
┌──────────────────────────────────────────────────────────┐
│              SuperOffice Service                         │
│                                                          │
│  Microsoft Graph API ──► Detect user location          │
│           ↓                                              │
│  NO → NOK, SE → SEK, US → USD (automatic!)             │
│           ↓                                              │
│  SuperOffice API ──► Fetch CRM data                     │
│           ↓                                              │
│  Currency Service ──► Convert all amounts               │
│           ↓                                              │
│  Return ready-to-display data in user's currency        │
└──────────────────────────────────────────────────────────┘
```

## Smart Features

### 1. **Single Sign-On (SSO)**
- Your frontend already authenticates users with Microsoft
- Same token is used for SuperOffice service
- **No separate login required**
- Users don't even know they're accessing SuperOffice

### 2. **Automatic Currency Detection**
```python
User Microsoft Profile → Location → Currency
    NO (Norway)        →   NOK
    SE (Sweden)        →   SEK
    US (United States) →   USD
    GB (UK)            →   GBP
    etc.
```

### 3. **Transparent Currency Conversion**
All monetary amounts are automatically converted:
```json
{
  "SaleId": 123,
  "Amount": 10000,           // Original amount
  "Currency": "USD",         // Original currency
  "AmountConverted": 105000, // Converted to NOK
  "_conversion": {
    "original_currency": "USD",
    "target_currency": "NOK",
    "rate": 10.5
  }
}
```

### 4. **AI-Ready Dashboard Data**
Single endpoint provides everything for AI dashboard generation:
```
GET /api/dashboard/data?organization_id={org_id}
↓
Returns:
  - User profile
  - Sales (with conversion)
  - Contacts
  - Companies
  - Summary stats
  - All in user's preferred currency
```

## Frontend Integration (Minimal Changes)

### Step 1: Add SuperOffice Card Component
```typescript
// Just add this component to your dashboard
<SuperOfficeCard 
  organizationId="your-org"
  microsoftToken={yourExistingToken} // From your current auth
/>
```

### Step 2: That's It!
- No new authentication flow
- No currency selection UI
- No complex data transformation
- Just display the data

## API Endpoints

### Main Endpoint for Dashboard
```
GET /api/dashboard/data?organization_id={org_id}
Headers: Authorization: Bearer {microsoft_token}

→ Returns all data in user's preferred currency
```

### User Profile with Currency
```
GET /api/user/profile?organization_id={org_id}
Headers: Authorization: Bearer {microsoft_token}

→ Returns user info + auto-detected currency preference
```

### Sales with Auto-Conversion
```
GET /api/sales/converted?organization_id={org_id}
Headers: Authorization: Bearer {microsoft_token}

→ Returns sales data converted to user's currency
```

## How It Works

### 1. **User Logs In** (Your existing frontend)
```typescript
// Your existing code (no changes)
const msalResponse = await msalInstance.loginPopup();
const microsoftToken = msalResponse.accessToken;
```

### 2. **Frontend Calls SuperOffice Service**
```typescript
// New: Just add this API call
const response = await fetch(
  'http://localhost:5006/api/dashboard/data?organization_id=my-org',
  {
    headers: {
      'Authorization': `Bearer ${microsoftToken}` // Same token!
    }
  }
);
const data = await response.json();
```

### 3. **Service Does the Magic**
1. Validates Microsoft token
2. Gets user profile from Microsoft Graph
3. Detects location → determines currency
4. Fetches SuperOffice data
5. Converts all amounts to user's currency
6. Returns structured data

### 4. **Display in Your UI**
```typescript
// Data is already converted!
<div>
  Total Sales: {data.summary.total_sales_value} {data.summary.currency}
</div>
```

## Configuration

### Backend (SuperOffice Service)
```env
# Use the SAME Microsoft credentials as your frontend
MICROSOFT_CLIENT_ID=same_as_frontend
MICROSOFT_CLIENT_SECRET=same_as_frontend
MICROSOFT_TENANT_ID=same_as_frontend

# Currency service (already running)
CURRENCY_SERVICE_URL=http://currency-service:5004
DEFAULT_CURRENCY=NOK
```

### Frontend (No changes needed)
Your existing Microsoft authentication continues to work as-is.

## Benefits

✅ **No Duplicate Auth**: Reuse existing Microsoft login  
✅ **Automatic Currency**: Detected from user location  
✅ **Zero Configuration**: No user settings needed  
✅ **AI Ready**: Perfect for AI dashboard generation  
✅ **Minimal Frontend Changes**: Just add one component  
✅ **Transparent**: Users don't see the complexity  

## Example: AI Dashboard Generation

```typescript
// Get all data in user's preferred currency
const dashboardData = await fetch('/api/dashboard/data?organization_id=...');

// Send to AI for dashboard creation
const aiPrompt = `
Create an executive dashboard with:
- Total sales: ${data.summary.total_sales_value} ${data.summary.currency}
- Sales by month chart
- Top performing contacts
- Recent deals
`;

const generatedDashboard = await ai.generate(aiPrompt);
// AI creates beautiful dashboard with correct currency!
```

## Deployment

1. **Services are already containerized**
   ```bash
   docker-compose up -d
   ```

2. **Configure Microsoft credentials**
   ```bash
   cp .env.example .env
   # Add your existing Microsoft app credentials
   ```

3. **Add SuperOffice card to frontend**
   ```typescript
   import { SuperOfficeCard } from './components/SuperOfficeCard';
   // Add to dashboard
   ```

4. **Done!**

## Security

- Microsoft token validation ensures only authenticated users
- No credentials stored in frontend
- All API calls are authenticated
- Currency conversion happens server-side

---

**The Magic**: Your users log in once with Microsoft, and automatically get SuperOffice CRM data in their preferred currency, ready for AI-powered dashboards. No extra steps, no configuration, just works! ✨
