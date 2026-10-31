# Frontend Integration Guide for SuperOffice Service

## Overview

Your frontend is **already authenticated with Microsoft**. This guide shows how to connect your existing Microsoft-authenticated frontend to the SuperOffice service to:

1. **Single Sign-On (SSO)**: Use the same Microsoft token for both frontend and SuperOffice
2. **Auto Currency Conversion**: Automatically convert SuperOffice data to user's preferred currency
3. **AI-Powered Dashboards**: Get structured data ready for AI dashboard generation

## Architecture

```
┌─────────────────────┐
│   Your Frontend     │
│ (Microsoft Auth)    │
│                     │
│ Microsoft Token ────┼──┐
└─────────────────────┘  │
                         │
                         ├──► Used for both:
                         │    - Frontend API calls
                         │    - SuperOffice service
                         │
┌─────────────────────┐  │
│  SuperOffice        │◄─┘
│    Service          │
│                     │
│ ┌─────────────────┐ │
│ │ Microsoft Graph │ │ ──► Get user location
│ └─────────────────┘ │      & currency preference
│                     │
│ ┌─────────────────┐ │
│ │ SuperOffice API │ │ ──► Fetch CRM data
│ └─────────────────┘ │
│                     │
│ ┌─────────────────┐ │
│ │ Currency Service│ │ ──► Auto convert amounts
│ └─────────────────┘ │
└─────────────────────┘
```

## Integration Steps

### Step 1: Frontend Already Has Microsoft Token

Your frontend already has a Microsoft access token from user login. Example:

```typescript
// Your existing frontend code (no changes needed)
const msalInstance = new PublicClientApplication(msalConfig);
const accounts = msalInstance.getAllAccounts();
const accessToken = await msalInstance.acquireTokenSilent({
  scopes: ["User.Read", "openid", "profile"],
  account: accounts[0]
}).then(response => response.accessToken);
```

### Step 2: Create SuperOffice Card Component

Create a React/Vue component to display SuperOffice data:

```typescript
// SuperOfficeCard.tsx (React example)
import { useState, useEffect } from 'react';

interface SuperOfficeCardProps {
  organizationId: string;
  microsoftToken: string; // From your existing auth
}

export const SuperOfficeCard: React.FC<SuperOfficeCardProps> = ({ 
  organizationId, 
  microsoftToken 
}) => {
  const [dashboardData, setDashboardData] = useState(null);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    fetchDashboardData();
  }, [organizationId]);

  const fetchDashboardData = async () => {
    try {
      // Call SuperOffice service with your existing Microsoft token
      const response = await fetch(
        `http://localhost:5006/api/dashboard/data?organization_id=${organizationId}`,
        {
          headers: {
            'Authorization': `Bearer ${microsoftToken}`,
            'Content-Type': 'application/json'
          }
        }
      );
      
      const data = await response.json();
      setDashboardData(data);
    } catch (error) {
      console.error('Failed to fetch SuperOffice data:', error);
    } finally {
      setLoading(false);
    }
  };

  if (loading) return <div>Loading SuperOffice data...</div>;

  return (
    <div className="superoffice-card">
      <h2>SuperOffice CRM</h2>
      
      <div className="summary">
        <h3>Summary ({dashboardData.user.currency_preference})</h3>
        <div className="stats">
          <div>Total Sales: {dashboardData.summary.total_sales}</div>
          <div>Sales Value: {dashboardData.summary.total_sales_value.toFixed(2)} {dashboardData.summary.currency}</div>
          <div>Contacts: {dashboardData.summary.total_contacts}</div>
          <div>Companies: {dashboardData.summary.total_companies}</div>
        </div>
      </div>

      <div className="recent-sales">
        <h3>Recent Sales</h3>
        <table>
          <thead>
            <tr>
              <th>Sale ID</th>
              <th>Title</th>
              <th>Amount (Original)</th>
              <th>Amount ({dashboardData.summary.currency})</th>
            </tr>
          </thead>
          <tbody>
            {dashboardData.sales.map(sale => (
              <tr key={sale.SaleId}>
                <td>{sale.SaleId}</td>
                <td>{sale.Heading}</td>
                <td>{sale.Amount} {sale.Currency}</td>
                <td>{sale.AmountConverted?.toFixed(2) || sale.Amount}</td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </div>
  );
};
```

### Step 3: Add SuperOffice Card to Your Dashboard

```typescript
// YourDashboard.tsx
import { SuperOfficeCard } from './components/SuperOfficeCard';

export const Dashboard = () => {
  const { microsoftToken } = useMsalAuth(); // Your existing auth hook
  const organizationId = "your-org-id"; // From your config or user profile

  return (
    <div className="dashboard">
      <h1>Dashboard</h1>
      
      {/* Your existing cards/widgets */}
      <YourExistingCards />
      
      {/* New SuperOffice card - just add this */}
      <SuperOfficeCard 
        organizationId={organizationId}
        microsoftToken={microsoftToken}
      />
    </div>
  );
};
```

## API Endpoints for Frontend

### 1. Get Dashboard Data (Main Endpoint)

**GET** `/api/dashboard/data?organization_id={org_id}`

Returns everything needed for AI dashboard:
- User profile with currency preference (auto-detected from location)
- Sales data (auto-converted to user's currency)
- Contacts and companies
- Summary statistics

**Response:**
```json
{
  "user": {
    "name": "John Doe",
    "email": "john@company.com",
    "currency_preference": "NOK"
  },
  "summary": {
    "total_sales": 45,
    "total_contacts": 120,
    "total_companies": 30,
    "total_sales_value": 1250000.50,
    "currency": "NOK"
  },
  "sales": [
    {
      "SaleId": 123,
      "Heading": "New Deal",
      "Amount": 10000,
      "Currency": "USD",
      "AmountConverted": 105000,
      "_conversion": {
        "original_currency": "USD",
        "target_currency": "NOK",
        "fields": {
          "Amount": {
            "original": 10000,
            "converted": 105000,
            "rate": 10.5
          }
        }
      }
    }
  ],
  "contacts": [...],
  "companies": [...],
  "supported_currencies": ["NOK", "SEK", "USD", "EUR", ...]
}
```

### 2. Get User Profile with Currency

**GET** `/api/user/profile?organization_id={org_id}`

Returns user profile with auto-detected currency preference.

### 3. Get Sales with Auto-Conversion

**GET** `/api/sales/converted?organization_id={org_id}&currency={currency}`

If currency is not specified, uses user's location-based preference.

### 4. Manual Currency Conversion

**POST** `/api/currency/convert`

```json
{
  "amount": 1000,
  "from_currency": "USD",
  "to_currency": "NOK"
}
```

## Smart Features

### 1. Automatic Currency Detection

The service automatically detects user's preferred currency from their Microsoft profile:

```
User Location (from Microsoft Graph)
    ↓
NO → NOK (Norwegian Krone)
SE → SEK (Swedish Krona)
US → USD (US Dollar)
UK → GBP (British Pound)
...etc
```

### 2. AI Dashboard Generation

The `/api/dashboard/data` endpoint provides structured data perfect for AI:

```typescript
// Example: Send data to your AI for dashboard generation
const dashboardData = await fetch('/api/dashboard/data?organization_id=...');

const aiPrompt = `
Create a dashboard with the following data:
${JSON.stringify(dashboardData, null, 2)}

Generate charts for:
1. Sales by month (in ${dashboardData.summary.currency})
2. Top contacts by engagement
3. Sales pipeline status
`;

// Send to your AI service
const generatedDashboard = await yourAI.generate(aiPrompt);
```

### 3. Real-time Currency Conversion

All amounts are converted in real-time using your currency service:
- Uses latest exchange rates
- Caches rates for performance
- Falls back gracefully if conversion fails

## Configuration

### Frontend Environment Variables

```env
# Add to your frontend .env
VITE_SUPEROFFICE_SERVICE_URL=http://localhost:5006
VITE_CURRENCY_SERVICE_URL=http://localhost:5004
```

### Backend Environment Variables

Already configured in `/apps/socrm_service/.env.example`:

```env
# Microsoft Auth (Primary)
MICROSOFT_CLIENT_ID=your_microsoft_client_id
MICROSOFT_CLIENT_SECRET=your_microsoft_client_secret
MICROSOFT_REDIRECT_URI=http://localhost:5006/api/auth/microsoft/callback
MICROSOFT_TENANT_ID=common

# Currency Service
CURRENCY_SERVICE_URL=http://currency-service:5004
DEFAULT_CURRENCY=NOK
```

## Example: Complete Integration Flow

```typescript
// 1. User logs in to your frontend with Microsoft
const msalResponse = await msalInstance.loginPopup({
  scopes: ["User.Read", "openid", "profile"]
});

// 2. Your frontend gets Microsoft token (already working)
const microsoftToken = msalResponse.accessToken;

// 3. Initialize SuperOffice connection (one-time setup)
const initSuperOffice = async () => {
  // Get authorization URL
  const authResponse = await fetch(
    '/api/auth/microsoft/authorize?organization_id=my-org',
    {
      headers: { 'Authorization': `Bearer ${microsoftToken}` }
    }
  );
  const { authorization_url } = await authResponse.json();
  
  // Open popup for user to authorize SuperOffice access
  window.open(authorization_url, 'superoffice-auth', 'width=600,height=700');
};

// 4. After authorization, fetch dashboard data
const getDashboardData = async () => {
  const response = await fetch(
    '/api/dashboard/data?organization_id=my-org',
    {
      headers: { 'Authorization': `Bearer ${microsoftToken}` }
    }
  );
  return await response.json();
};

// 5. Display in your UI
const dashboardData = await getDashboardData();
// Currency is already converted based on user's location!
```

## Benefits

✅ **Single Sign-On**: One Microsoft login for everything  
✅ **Auto Currency**: No manual selection needed  
✅ **AI Ready**: Structured data for AI dashboard generation  
✅ **Real-time**: Live data from SuperOffice CRM  
✅ **Seamless**: Minimal changes to existing frontend  

## Testing

1. **Start services:**
   ```bash
   # Start currency service
   cd apps/currency_service
   docker-compose up -d
   
   # Start SuperOffice service
   cd ../socrm_service
   docker-compose up -d
   ```

2. **Test currency detection:**
   ```bash
   curl "http://localhost:5006/api/user/profile?organization_id=test" \
     -H "Authorization: Bearer YOUR_MICROSOFT_TOKEN"
   ```

3. **Test dashboard data:**
   ```bash
   curl "http://localhost:5006/api/dashboard/data?organization_id=test" \
     -H "Authorization: Bearer YOUR_MICROSOFT_TOKEN"
   ```

## Support

For issues or questions:
1. Check service logs: `docker-compose logs -f`
2. Verify Microsoft token is valid
3. Ensure currency service is running
4. Check network connectivity between services

---

**Ready to use!** Just add the SuperOfficeCard component to your existing dashboard.
