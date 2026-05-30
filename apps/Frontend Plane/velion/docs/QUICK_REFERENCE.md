# Velion UI Refactoring - Quick Reference Card

## 🚀 Quick Start

```typescript
// 1. Wrap your app
import { SessionProvider } from '@/lib/session-context'
// In src/app/layout.tsx: <SessionProvider>{children}</SessionProvider>

// 2. Use in page
import { MasterPageLayout, StatCard, GridContainer } from '@/components/shared'
import { useSession } from '@/lib/session-context'

export default function Page() {
  const { user } = useSession()
  
  return (
    <MasterPageLayout title="Projects">
      <GridContainer cols={3}>
        <StatCard label="Active" value={42} trend="up" change={5} />
      </GridContainer>
    </MasterPageLayout>
  )
}
```

## 📦 Import Paths

```typescript
// Components
import { MasterPageLayout, StatCard, CommonCard, Badge } from '@/components/shared'
import { EmptyState, LoadingState, ErrorState } from '@/components/shared'

// Hooks & Context
import { useSession } from '@/lib/session-context'
import { useFetch, useOptimisticMutation } from '@/lib/common-hooks'

// API & Utils
import { apiGet, apiPost, apiPut, apiDelete } from '@/lib/api-utils'
import { THEME, formatCompactNumber, truncateText } from '@/lib/design-tokens'

// Types
import type { User, BaseMetric, ViewConfig } from '@/lib/common-types'
```

## 🎨 Design Tokens

```typescript
import { THEME } from '@/lib/design-tokens'

// Colors
THEME.colors.background         // Page background
THEME.colors['text-primary']    // Main text
THEME.colors.border             // Borders
THEME.colors.success            // Success color

// Spacing
THEME.spacing.md, .lg, .xl       // Consistent spacing

// Use in styles
className={`rounded-[22px] bg-[${THEME.colors.surface}]`}
```

## 🧩 Common Components Cheat Sheet

| Component | Usage | Props |
|-----------|-------|-------|
| **MasterPageLayout** | Page wrapper | `title`, `description`, `variant`, `showHeader` |
| **StatCard** | Metric display | `label`, `value`, `unit`, `trend`, `change` |
| **CommonCard** | Content box | `interactive`, `hoverable`, `shadow` |
| **SectionCard** | Grouped content | `title`, `subtitle`, `action` |
| **GridContainer** | Responsive grid | `cols` (2,3,4), `gap` |
| **Badge** | Status label | `variant`, `size` |
| **Button** | Button | `variant`, `size`, `isLoading` |
| **LoadingState** | Skeleton | `itemCount` |
| **EmptyState** | No data | `title`, `description`, `action` |
| **ErrorState** | Error display | `title`, `message`, `onRetry` |

## 🔗 API Usage

```typescript
// GET
const { data, error } = await apiGet('/api/projects')

// POST
const result = await apiPost('/api/projects', { name: 'New' })

// PUT
await apiPut(`/api/projects/${id}`, { name: 'Updated' })

// DELETE
await apiDelete(`/api/projects/${id}`)
```

## ⚡ Hooks

```typescript
// Session
const { user, isLoading, error } = useSession()

// Data Fetching
const { data, isLoading, error, refetch } = useFetch('/api/data', {
  skip: !user,
  refetchInterval: 30000
})

// Mutations
const { mutate, isPending, error } = useOptimisticMutation(
  data => apiPost('/api/data', data)
)
```

## 🎯 Common Patterns

### Pattern 1: Page with List
```typescript
export default async function Page() {
  const session = await getSession()
  if (!session) redirect('/auth/login')
  
  return (
    <MasterPageLayout title="Items">
      <GridContainer cols={2}>
        {items.map(item => (
          <StatCard key={item.id} label={item.name} value={item.count} />
        ))}
      </GridContainer>
    </MasterPageLayout>
  )
}
```

### Pattern 2: With Loading State
```typescript
function ItemList() {
  const { data, isLoading, error } = useFetch('/api/items')
  
  if (isLoading) return <LoadingState />
  if (error) return <ErrorState onRetry={refetch} />
  if (!data?.length) return <EmptyState title="No items" />
  
  return <div>{/* render items */}</div>
}
```

### Pattern 3: Create/Update
```typescript
function CreateItem() {
  const { mutate, isPending } = useOptimisticMutation(
    data => apiPost('/api/items', data)
  )
  
  const handleSubmit = async (formData) => {
    try {
      await mutate(formData)
      // Show success
    } catch (err) {
      // Show error
    }
  }
  
  return <form onSubmit={handleSubmit}>...</form>
}
```

## 🎨 Styling

```typescript
// Use design tokens
className={`
  rounded-[${THEME.borderRadius['2xl']}]
  bg-[${THEME.colors.surface}]
  border border-[${THEME.colors.border}]
  shadow-[${THEME.shadow.lg}]
  px-[${THEME.spacing.lg}]
  text-[${THEME.colors['text-primary']}]
`}

// Or use TailwindCSS directly
className="rounded-2xl bg-white border border-[#E6E8EF] shadow-lg px-6 text-[#2F3138]"
```

## 📱 Responsive Grid

```typescript
// 3 columns on desktop, 2 on tablet, 1 on mobile
<GridContainer cols={3} gap="md">
  {items.map(item => <StatCard key={item.id} {...item} />)}
</GridContainer>
```

## 🛠️ TypeScript

```typescript
// Define types
import type { BaseEntity, User } from '@/lib/common-types'

interface Project extends BaseEntity {
  name: string
  owner: User
}

// Use in component
function ProjectCard({ project }: { project: Project }) {
  return <CommonCard>{project.name}</CommonCard>
}
```

## 🚨 Error Handling

```typescript
// API errors are caught automatically
const { error } = await apiGet('/api/data')
if (error) {
  console.error(error) // Retry happened automatically
}

// Use ErrorState
if (error) {
  return <ErrorState onRetry={() => refetch()} />
}
```

## 📊 Common Metrics

```typescript
// Activity card
<StatCard 
  label="Messages Today"
  value={142}
  unit="msgs"
  trend="up"
  change={12}
/>

// Status card
<StatCard
  label="System Health"
  value="98%"
  trend="stable"
/>
```

## 🎓 Learning Resources

- **Full Guide:** `docs/REFACTORING_GUIDE.md`
- **Integration:** `docs/INTEGRATION_STEPS.md`
- **Architecture:** `docs/ARCHITECTURE_DIAGRAM.md`
- **Checklist:** `docs/IMPLEMENTATION_CHECKLIST.md`

## 🔍 Debugging

```typescript
// Check session
const { user, isLoading, error } = useSession()
console.log('Current user:', user)
console.log('Loading:', isLoading)
console.log('Error:', error)

// Check API response
const response = await apiGet('/api/data')
console.log('Response:', response)
console.log('Data:', response.data)
console.log('Error:', response.error)

// Check rendering
if (isLoading) console.log('Still loading...')
if (error) console.log('Error:', error.message)
if (!data) console.log('No data')
```

## ⚙️ Configuration

```typescript
// API timeout (default: 10000ms)
await apiGet('/api/data', { timeout: 5000 })

// API retries (default: 2)
await apiGet('/api/data', { retries: 3 })

// Fetch refetch interval (default: manual)
useFetch('/api/data', { refetchInterval: 30000 })

// Skip fetch
useFetch('/api/data', { skip: !user })
```

## 📋 Folder Structure

```
src/
├── lib/
│   ├── design-tokens.ts      ← Colors, spacing, typography
│   ├── common-utils.ts       ← Formatting functions
│   ├── api-utils.ts          ← API wrapper
│   ├── common-types.ts       ← Shared types
│   ├── session-context.tsx   ← Auth context
│   └── common-hooks.ts       ← useFetch, useOptimisticMutation
└── components/shared/
    ├── layouts.tsx           ← MasterPageLayout, GridContainer
    ├── CommonCard.tsx        ← CommonCard, SectionCard, StatCard
    ├── StateComponents.tsx    ← EmptyState, LoadingState, ErrorState
    ├── CommonUI.tsx          ← Badge, Button, Divider
    └── index.ts              ← Barrel exports
```

---

**Last Updated:** April 4, 2026  
**Status:** Ready for Integration  
**Questions?** See `docs/REFACTORING_GUIDE.md`

