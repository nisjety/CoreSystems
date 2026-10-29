# Contifico Service - Authentication Configuration

## Current Status

✅ **Service is working** with mock data fallback  
❌ **Real Contifico API** returns: `Unauthorized: Invalid API credentials`

## Issue

The test credentials in `.env` are returning 401 Unauthorized from the Contifico API:

```env
CONTIFICO_API_KEY=FrguR1kDpFHaXHLQwplZ2CwTX3p8p9XHVTnukL98V5U
CONTIFICO_API_TOKEN=02914770-4a13-45f0-bfe3-c2e4666cdbcf
CONTIFICO_API_BASE_URL=https://api.contifico.com/sistema/api/v1
```

These are documented as Contifico's test credentials (`APIPRUEBA/APIPRUEBA`), but they may be:
- Outdated
- Require additional activation
- Need a different authentication method
- Region-specific (Ecuador vs other regions)

## How to Fix

### Option 1: Get Real Contifico Credentials

1. Log in to your Contifico account at https://www.contifico.com
2. Navigate to **Settings** → **API** or **Integrations**
3. Generate new API Key and Token
4. Update the `.env` file with your credentials:

```bash
# Production credentials
CONTIFICO_API_KEY=your_real_api_key_here
CONTIFICO_API_TOKEN=your_real_api_token_here
```

5. Restart the service:

```bash
cd /opt/aquatiq-integrations
docker compose -f docker-compose.prod.yml restart contifico-service
```

### Option 2: Contact Contifico Support

If you don't have access to API credentials:

1. Contact Contifico support: soporte@contifico.com
2. Request API access for your account
3. Verify your plan includes API access (may require Plus or Premium plan)
4. Documentation: http://contifico.github.io/

### Option 3: Continue with Mock Data (Testing Only)

The service currently returns mock data when API authentication fails. This allows:
- UI/UX testing
- Frontend development
- Demo purposes

**Mock data includes:**
- 3 products
- 2 categories  
- 2 warehouses
- 2 brands
- 2 documents
- 2 inventory movements

## Testing Authentication

After updating credentials, test with:

```bash
# Test direct service
curl http://31.97.38.31:8006/api/data/products | jq .

# Check logs for "Returning mock data" warnings
docker logs aquatiq-contifico-service --tail 20 | grep "mock data"
```

If authentication works, you should see real data without "mock data" warnings in logs.

## Authentication Headers

The client currently sends:

```python
session.headers.update({
    "Authorization": self.api_key,
    "Content-Type": "application/json",
    "Accept": "application/json",
    "api_key": self.api_key,
    "api_token": self.api_token
})
```

If Contifico requires a different format, update `/apps/contifico_service/clients/contifico_client.py` line 67-74.

## Next Steps

1. ✅ Frontend path fixed (removed duplicate `/api/`)
2. ✅ Mock data fallback working
3. ⏳ Obtain real Contifico API credentials
4. ⏳ Update `.env` files (local + production)
5. ⏳ Test real data sync
6. ⏳ Implement GCP sync functionality

## Files Modified

- `apps/frontend/src/app/(dashboard)/erp-connector/contifico/data/page.tsx` - Fixed endpoint paths
- `apps/contifico_service/clients/contifico_client.py` - Added token to headers
- `apps/contifico_service/app.py` - Mock data fallback already implemented

## Production Deployment

```bash
# Update .env.production on VPS
ssh root@31.97.38.31
cd /opt/aquatiq-integrations/apps/contifico_service
nano .env.production  # Add real credentials

# Restart service
cd /opt/aquatiq-integrations
docker compose -f docker-compose.prod.yml restart contifico-service
```
