# ✅ Velion Frontend — Third-Party Service Integrations

> **Scope:** this document covers velion's integration with **externally-hosted**
> third-party services only: **Zammad** (ticketing), **Nango** (API connectors),
> and **Nohu** (workflows). For Control Plane / Application Plane integration
> (auth-core, user-core, org-core, billing-core, session-core, notification-core,
> convex-core, the L5 ingress policy) see **[`velion-gap.md`](./velion-gap.md)** —
> the living gap log + architecture reference. The file was previously named
> `VELION_INTEGRATION.md`; renamed 2026-05-11 per G22 to stop new contributors
> assuming Control Plane wiring lives here.

Successfully integrated **Zammad** (ticketing), **Nango** (API connectors), and **Nohu** (workflows) with Velion.

## 📦 What Was Created

### Client Libraries (3 files)
- **zammad-client.ts** - Customer support ticketing API
- **nango-client.ts** - Universal API connector platform
- **nohu-client.ts** - Workflow orchestration engine

### React Hooks (3 files)
- **useZammad.ts** - Zammad integration in components
- **useNango.ts** - Nango OAuth and connectors
- **useNohu.ts** - Workflow execution with auto-polling

### API Proxy Routes (3 files)
- **/api/external/zammad/[...path]** - Secure Zammad proxy
- **/api/external/nango/[...path]** - Secure Nango proxy
- **/api/external/nohu/[...path]** - Secure Nohu proxy

## 🚀 Quick Start

### 1. Add Credentials to .env
```bash
# Server-side (Docker hostnames)
EXTERNAL_ZAMMAD_API_URL=http://zammad-api:3012/api/v1
EXTERNAL_ZAMMAD_TOKEN=<token>

EXTERNAL_NANGO_API_URL=http://nango-api:3013
EXTERNAL_NANGO_API_KEY=<key>

EXTERNAL_NOHU_API_URL=http://nohu-api:3014
EXTERNAL_NOHU_API_KEY=<key>
```

### 2. Use in Components
```tsx
'use client'
import { useZammadTickets } from '@/lib/hooks/useZammad'

export default function Dashboard() {
  const { tickets, loading } = useZammadTickets()
  
  if (loading) return <div>Loading...</div>
  return <div>{tickets.length} tickets</div>
}
```

## 📚 API Reference

### Zammad
- `useZammadTickets()` - List tickets
- `useCreateZammadTicket()` - Create ticket
- `useZammadHealth()` - Health check

### Nango
- `useNangoIntegrations()` - List 500+ integrations
- `useNangoConnections()` - Manage connections
- `useNangoOAuth()` - OAuth flows
- `useNangoHealth()` - Health check

### Nohu
- `useNohuWorkflows()` - List workflows
- `useExecuteWorkflow()` - Run workflow
- `useNohuExecution()` - Monitor execution (auto-polls)
- `useNohuHealth()` - Health check

## 🔒 Security

- ✅ Server-side credential injection via proxy routes
- ✅ No API keys exposed to browser
- ✅ CORS handled at proxy layer
- ✅ Environment-based configuration

## 📄 Files Structure

```
velion/
├── src/lib/
│   ├── clients/
│   │   ├── zammad-client.ts (2.6K)
│   │   ├── nango-client.ts (2.9K)
│   │   └── nohu-client.ts (3.0K)
│   └── hooks/
│       ├── useZammad.ts (2.3K)
│       ├── useNango.ts (3.2K)
│       └── useNohu.ts (3.5K)
├── src/app/api/external/
│   ├── zammad/[...path]/route.ts
│   ├── nango/[...path]/route.ts
│   └── nohu/[...path]/route.ts
└── VELION_THIRD_PARTY_INTEGRATIONS.md (this file)
```

## 🧪 Testing

### Health Checks
```tsx
const { healthy } = useZammadHealth()
const { healthy: nangoOk } = useNangoHealth()
const { healthy: nohuOk } = useNohuHealth()
```

### Fetch Resources
```tsx
const { tickets } = useZammadTickets()
const { integrations } = useNangoIntegrations()
const { workflows } = useNohuWorkflows()
```

### Create Resources
```tsx
const { create } = useCreateZammadTicket()
await create({ title: 'Bug', group: 'support', customer_email: 'user@example.com' })
```

## 📝 Environment Variables

| Variable | Server-side | Client-side | Purpose |
|----------|------------|------------|---------|
| EXTERNAL_ZAMMAD_API_URL | Docker hostname | localhost | Zammad base URL |
| EXTERNAL_ZAMMAD_TOKEN | (secret) | (localStorage) | Authentication |
| EXTERNAL_NANGO_API_URL | Docker hostname | localhost | Nango base URL |
| EXTERNAL_NANGO_API_KEY | (secret) | (localStorage) | Authentication |
| EXTERNAL_NOHU_API_URL | Docker hostname | localhost | Nohu base URL |
| EXTERNAL_NOHU_API_KEY | (secret) | (localStorage) | Authentication |

## 🔧 Advanced Usage

### Custom Client Initialization
```tsx
import { ZammadClient } from '@/lib/clients/zammad-client'

const client = new ZammadClient('http://custom-host:3012', 'token')
const tickets = await client.getTickets()
```

### OAuth Flow with Nango
```tsx
const { initiateOAuth } = useNangoOAuth()

const handleConnect = async () => {
  const url = await initiateOAuth(
    'github',
    `${window.location.origin}/auth/callback`
  )
  window.location.href = url
}
```

### Workflow Monitoring
```tsx
const { execute } = useExecuteWorkflow()
const { execution } = useNohuExecution(executionId)

// useNohuExecution auto-polls every 2 seconds while running
if (execution?.status === 'completed') {
  console.log('Done:', execution.output)
}
```

## 🐛 Troubleshooting

| Issue | Solution |
|-------|----------|
| "API key not found" | Check .env has EXTERNAL_*_TOKEN/API_KEY |
| CORS errors | Ensure requests use /api/external/* routes |
| Connection refused | Verify services on 3012/3013/3014 |
| 404 on health | Ensure client initialized with baseUrl |

## ✅ Integration Complete

All components ready for production use with:
- ✅ Full TypeScript support
- ✅ Error handling
- ✅ Loading states
- ✅ Auto-refresh capabilities
- ✅ Security best practices

See `/memories/repo/VELION_INTEGRATION_SUMMARY.md` for full documentation.
