# Verevon UI Refactoring Guide

## Overview

This refactoring centralizes common UI patterns, utilities, and types across the Verevon application. The goal is to ensure consistency, reduce code duplication, and improve maintainability.

## New Folder Structure

```
src/
├── lib/
│   ├── design-tokens.ts          # Centralized theme colors, spacing, typography
│   ├── common-utils.ts           # Reusable utility functions
│   ├── api-utils.ts              # API fetching wrapper with retry logic
│   ├── common-types.ts           # Shared TypeScript types and interfaces
│   ├── session-context.tsx       # Session provider and hook
│   └── common-hooks.ts           # Custom React hooks (useFetch, useOptimisticMutation)
└── components/
    └── shared/
        ├── index.ts              # Barrel exports
        ├── layouts.tsx           # MasterPageLayout, GridContainer
        ├── CommonCard.tsx        # CommonCard, SectionCard, StatCard
        ├── StateComponents.tsx    # EmptyState, LoadingState, ErrorState
        └── CommonUI.tsx          # Badge, Button, Divider
```

## Core Components

### 1. Design Tokens (`lib/design-tokens.ts`)

Centralized theme values for consistent styling:

```typescript
import { THEME } from '@/lib/design-tokens';

// Colors
THEME.colors.background      // '#F7F7FA'
THEME.colors.surface         // '#FFFFFF'
THEME.colors.border          // '#E5E7EE'
THEME.colors['text-primary'] // '#2F3138'

// Spacing
THEME.spacing.md             // '1rem'
THEME.spacing.lg             // '1.5rem'

// Typography
THEME.typography['3xl']      // { size, weight, lineHeight }

// Shadows
THEME.shadow.xl              // '0 12px 30px rgba(...)'
```

### 2. Common Utilities (`lib/common-utils.ts`)

Reusable functions for formatting and styling:

```typescript
import { formatCompactNumber, truncateText, classNames } from '@/lib/common-utils';

formatCompactNumber(1500)    // '1.5K'
truncateText('Long text...', 20)  // 'Long text...'
classNames('px-4', isActive && 'bg-blue')  // Conditional classnames
```

### 3. API Utilities (`lib/api-utils.ts`)

Consistent API fetching with retry logic and error handling:

```typescript
import { apiGet, apiPost } from '@/lib/api-utils';

const { data, error } = await apiGet('/api/projects');
const result = await apiPost('/api/projects', { name: 'New Project' });
```

### 4. Session Context (`lib/session-context.tsx`)

Centralized session management:

```typescript
import { useSession } from '@/lib/session-context';

function MyComponent() {
  const { user, isLoading, error } = useSession();
  
  if (isLoading) return <LoadingState />;
  if (!user) return <EmptyState />;
  
  return <div>Welcome {user.name}</div>;
}
```

### 5. Common Hooks (`lib/common-hooks.ts`)

**`useFetch`** - Data fetching with session validation:
```typescript
const { data, isLoading, error, refetch } = useFetch('/api/projects');
```

**`useOptimisticMutation`** - Optimistic updates:
```typescript
const { mutate, isPending, error } = useOptimisticMutation(
  (data) => apiPost('/api/projects', data)
);
```

## Shared Components

### 1. Layouts

**MasterPageLayout** - Standard page structure:
```typescript
import { MasterPageLayout } from '@/components/shared';

export default function ProjectsPage() {
  return (
    <MasterPageLayout 
      title="Projects"
      description="Manage your projects"
      variant="default"
    >
      {/* Content */}
    </MasterPageLayout>
  );
}
```

**GridContainer** - Responsive grid:
```typescript
<GridContainer cols={3} gap="md">
  <Card>Item 1</Card>
  <Card>Item 2</Card>
  <Card>Item 3</Card>
</GridContainer>
```

### 2. Cards

**CommonCard** - Base card component:
```typescript
import { CommonCard } from '@/components/shared';

<CommonCard hoverable shadow="xl">
  Content here
</CommonCard>
```

**StatCard** - Display metrics:
```typescript
import { StatCard } from '@/components/shared';

<StatCard 
  label="Active Projects"
  value={42}
  unit="items"
  trend="up"
  change={12}
/>
```

**SectionCard** - Group related content:
```typescript
<SectionCard title="Overview" subtitle="Key metrics">
  Content
</SectionCard>
```

### 3. State Components

**EmptyState** - No data:
```typescript
import { EmptyState } from '@/components/shared';

<EmptyState 
  title="No projects yet"
  description="Create your first project to get started"
/>
```

**LoadingState** - Loading indicators:
```typescript
<LoadingState title="Loading projects..." itemCount={6} />
```

**ErrorState** - Error handling:
```typescript
<ErrorState 
  title="Failed to load"
  message="Please try again"
  onRetry={() => refetch()}
/>
```

### 4. UI Elements

**Badge** - Status labels:
```typescript
import { Badge } from '@/components/shared';

<Badge variant="success">Active</Badge>
<Badge variant="warning">Pending</Badge>
<Badge variant="error">Failed</Badge>
```

**Button** - Consistent buttons:
```typescript
import { Button } from '@/components/shared';

<Button variant="primary" size="md" onClick={handleClick}>
  Create Project
</Button>
```

**Divider** - Visual separators:
```typescript
import { Divider } from '@/components/shared';

<Divider variant="horizontal" />
```

## Migration Guide

### Before (Duplicated code in multiple sections)
```typescript
// components/overview/OverviewLanding.tsx
const MetricCard = ({ metric }) => (
  <div className="rounded-[22px] border border-[#E6E8EF]...">
    {/* repeated styling */}
  </div>
);

// components/dashboard/DashboardCard.tsx
const DashboardCard = ({ data }) => (
  <div className="rounded-[22px] border border-[#E6E8EF]...">
    {/* same styling */}
  </div>
);
```

### After (Centralized component)
```typescript
// Use StatCard from shared
import { StatCard, MasterPageLayout } from '@/components/shared';

export default function ProjectsPage() {
  return (
    <MasterPageLayout title="Projects">
      <StatCard 
        label="Active Projects"
        value={42}
        trend="up"
        change={5}
      />
    </MasterPageLayout>
  );
}
```

## Type System

Common types are in `lib/common-types.ts`:

```typescript
import type { BaseEntity, User, BaseMetric, ViewConfig } from '@/lib/common-types';

interface Project extends BaseEntity {
  name: string;
  description: string;
  owner: User;
}
```

## Best Practices

1. **Use MasterPageLayout** for all dashboard pages
2. **Use design tokens** for styling consistency
3. **Use common hooks** for data fetching
4. **Use shared components** instead of creating new ones
5. **Import from `@/components/shared`** barrel export for cleaner imports
6. **Follow the API utility pattern** for all API calls
7. **Always validate session** before showing content

## Examples

See the following files for complete examples:
- `/src/app/(dashboard)/projects/page.tsx` - Project page
- `/src/app/(dashboard)/tasks/page.tsx` - Task page
- `/src/app/(dashboard)/agents/page.tsx` - Agent page

## Adding New Shared Components

1. Create component in `src/components/shared/`
2. Export from `src/components/shared/index.ts`
3. Document usage in this guide
4. Update any related dashboard pages

## Questions or Issues?

If a component is being used in multiple places and isn't in the shared library yet, consider:
1. Moving it to `src/components/shared/`
2. Extracting the pattern into a new shared component
3. Creating a new utility function if it's logic-based
