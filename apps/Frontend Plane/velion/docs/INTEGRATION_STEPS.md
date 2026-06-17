# Integration Steps for Refactored Components

## 1. Update Root Layout (`src/app/layout.tsx`)

Wrap your app with SessionProvider:

```typescript
import { SessionProvider } from '@/lib/session-context';
import './globals.css';

export default function RootLayout({ children }: { children: React.ReactNode }) {
  return (
    <html lang="en">
      <body>
        <SessionProvider>
          {children}
        </SessionProvider>
      </body>
    </html>
  );
}
```

## 2. Modernize Dashboard Pages

Example: Convert `/dashboard/projects/page.tsx`

```typescript
import { redirect } from 'next/navigation';
import { MasterPageLayout, GridContainer, StatCard, EmptyState } from '@/components/shared';
import { useFetch } from '@/lib/common-hooks';
import { getSession } from '@/lib/session-context';

async function getSession() {
  const res = await fetch('http://localhost:3000/api/auth/session', { cache: 'no-store' });
  return res?.ok ? res.json() : null;
}

export default async function ProjectsPage() {
  const session = await getSession();
  
  if (!session?.user?.id) {
    redirect('/auth/login');
  }

  // Server-side data fetching here
  
  return (
    <MasterPageLayout 
      title="Projects"
      description="Manage all your organizational projects"
    >
      <GridContainer cols={3} gap="md">
        {/* Fill with project cards */}
      </GridContainer>
    </MasterPageLayout>
  );
}
```

## 3. Create Common Table Component

For list views across sections:

```typescript
// src/components/shared/CommonTable.tsx
import { ComponentPropsWithoutRef } from 'react';

interface TableColumn {
  key: string;
  label: string;
  render?: (value: any, row: any) => React.ReactNode;
  width?: string;
}

interface CommonTableProps<T> extends ComponentPropsWithoutRef<'table'> {
  columns: TableColumn[];
  data: T[];
  isLoading?: boolean;
}

export function CommonTable<T>({ 
  columns, 
  data, 
  isLoading,
  ...props 
}: CommonTableProps<T>) {
  if (isLoading) {
    return <div className="p-4 text-center">Loading...</div>;
  }

  if (data.length === 0) {
    return <div className="p-4 text-center text-gray-500">No data</div>;
  }

  return (
    <table className="w-full" {...props}>
      <thead className="border-b border-[#E6E8EF] bg-[#F7F8FB]">
        <tr>
          {columns.map((col) => (
            <th
              key={col.key}
              className="px-4 py-3 text-left text-sm font-semibold text-[#2F3138]"
              style={{ width: col.width }}
            >
              {col.label}
            </th>
          ))}
        </tr>
      </thead>
      <tbody>
        {data.map((row, idx) => (
          <tr key={idx} className="border-b border-[#E6E8EF] hover:bg-[#F7F8FB]">
            {columns.map((col) => (
              <td key={col.key} className="px-4 py-3 text-sm text-[#707480]">
                {col.render ? col.render((row as any)[col.key], row) : (row as any)[col.key]}
              </td>
            ))}
          </tr>
        ))}
      </tbody>
    </table>
  );
}
```

## 4. Create Modal Component

For consistent modal dialogs:

```typescript
// src/components/shared/CommonModal.tsx
import { ComponentPropsWithoutRef, ReactNode } from 'react';
import { Button } from './CommonUI';

interface CommonModalProps extends ComponentPropsWithoutRef<'div'> {
  isOpen: boolean;
  title: string;
  onClose: () => void;
  onSubmit?: () => void;
  submitLabel?: string;
  children: ReactNode;
}

export function CommonModal({
  isOpen,
  title,
  onClose,
  onSubmit,
  submitLabel = 'Submit',
  children,
}: CommonModalProps) {
  if (!isOpen) return null;

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/50">
      <div className="rounded-[22px] bg-white p-6 shadow-xl md:w-1/2">
        <div className="mb-4 flex items-center justify-between">
          <h2 className="text-[20px] font-semibold text-[#2F3138]">{title}</h2>
          <button
            onClick={onClose}
            className="text-[#9A9EAA] hover:text-[#2F3138]"
          >
            ✕
          </button>
        </div>

        <div className="mb-6">{children}</div>

        <div className="flex gap-3 justify-end">
          <Button variant="secondary" onClick={onClose}>
            Cancel
          </Button>
          {onSubmit && (
            <Button variant="primary" onClick={onSubmit}>
              {submitLabel}
            </Button>
          )}
        </div>
      </div>
    </div>
  );
}
```

## 5. Create Common Form Component

For handling form states consistently:

```typescript
// src/components/shared/CommonForm.tsx
import { ComponentPropsWithoutRef } from 'react';
import { Button } from './CommonUI';

interface FormField {
  name: string;
  label: string;
  type?: 'text' | 'email' | 'password' | 'textarea' | 'select';
  placeholder?: string;
  required?: boolean;
  options?: { label: string; value: string }[];
}

interface CommonFormProps extends ComponentPropsWithoutRef<'form'> {
  fields: FormField[];
  onSubmit: (data: Record<string, any>) => void;
  submitLabel?: string;
  isLoading?: boolean;
}

export function CommonForm({
  fields,
  onSubmit,
  submitLabel = 'Submit',
  isLoading = false,
  ...props
}: CommonFormProps) {
  const handleSubmit = async (e: React.FormEvent<HTMLFormElement>) => {
    e.preventDefault();
    const formData = new FormData(e.currentTarget);
    const data = Object.fromEntries(formData);
    onSubmit(data);
  };

  return (
    <form onSubmit={handleSubmit} {...props}>
      <div className="space-y-4">
        {fields.map((field) => (
          <div key={field.name}>
            <label className="block text-sm font-medium text-[#2F3138] mb-2">
              {field.label}
              {field.required && <span className="text-red-600">*</span>}
            </label>

            {field.type === 'textarea' ? (
              <textarea
                name={field.name}
                placeholder={field.placeholder}
                required={field.required}
                className="w-full rounded-lg border border-[#E6E8EF] px-4 py-2 focus:border-blue-500 focus:outline-none"
              />
            ) : field.type === 'select' ? (
              <select
                name={field.name}
                required={field.required}
                className="w-full rounded-lg border border-[#E6E8EF] px-4 py-2 focus:border-blue-500 focus:outline-none"
              >
                {field.options?.map((opt) => (
                  <option key={opt.value} value={opt.value}>
                    {opt.label}
                  </option>
                ))}
              </select>
            ) : (
              <input
                type={field.type || 'text'}
                name={field.name}
                placeholder={field.placeholder}
                required={field.required}
                className="w-full rounded-lg border border-[#E6E8EF] px-4 py-2 focus:border-blue-500 focus:outline-none"
              />
            )}
          </div>
        ))}
      </div>

      <Button 
        type="submit" 
        variant="primary" 
        isLoading={isLoading}
        className="mt-6 w-full"
      >
        {submitLabel}
      </Button>
    </form>
  );
}
```

## 6. Export New Components

Update `src/components/shared/index.ts`:

```typescript
// Existing exports...
export { CommonTable } from './CommonTable';
export { CommonModal } from './CommonModal';
export { CommonForm } from './CommonForm';
```

## Testing the Integration

1. **Test SessionProvider**:
   ```bash
   pnpm dev
   # Navigate to /dashboard/projects
   # Should redirect to /auth/login if not authenticated
   ```

2. **Test Shared Components**:
   ```typescript
   import { MasterPageLayout, StatCard } from '@/components/shared';
   // Should import without errors
   ```

3. **Test API Utils**:
   ```typescript
   import { apiGet } from '@/lib/api-utils';
   const data = await apiGet('/api/projects');
   ```

## Migration Checklist

- [ ] Update `src/app/layout.tsx` with SessionProvider
- [ ] Add new shared components to `src/components/shared/`
- [ ] Update dashboard pages to use MasterPageLayout
- [ ] Replace duplicate MetricCard/Card components with StatCard/CommonCard
- [ ] Update API calls to use api-utils
- [ ] Remove old/duplicate component files
- [ ] Test all dashboard sections
- [ ] Run `pnpm build` to check for TypeScript errors
- [ ] Update component documentation
