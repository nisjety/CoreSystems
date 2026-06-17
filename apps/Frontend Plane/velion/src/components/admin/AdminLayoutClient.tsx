'use client'

import Link from 'next/link'
import { usePathname } from 'next/navigation'

const adminNav = [
  { name: 'Dashboard', href: '/admin' },
  { name: 'Users', href: '/admin/users' },
  { name: 'Organizations', href: '/admin/organizations' },
  { name: 'Billing', href: '/admin/billing' },
  // Phase A · A3 — cross-plane health roll-up, polls each plane's
  // /healthz over inter-plane-bus from a single proxy route.
  { name: 'System health', href: '/admin/system-health' },
]

interface AdminLayoutClientProps {
  children: React.ReactNode
  user: {
    name: string
    email: string
  }
}

export function AdminLayoutClient({ children, user }: AdminLayoutClientProps) {
  const pathname = usePathname()

  return (
    <div className="flex min-h-screen">
      {/* Sidebar */}
      <aside className="w-64 border-r bg-gray-50 dark:bg-gray-900">
        <div className="p-6">
          <h2 className="text-xl font-bold">Admin Panel</h2>
          <p className="text-xs text-muted-foreground mt-1">{user.name}</p>
        </div>
        <nav className="space-y-1 px-3">
          {adminNav.map((item) => {
            const isActive = pathname === item.href
            return (
              <Link
                key={item.href}
                href={item.href}
                className={`block px-3 py-2 rounded-md text-sm font-medium transition-colors ${
                  isActive
                    ? 'bg-gray-900 text-white dark:bg-gray-100 dark:text-gray-900'
                    : 'text-gray-700 hover:bg-gray-200 dark:text-gray-300 dark:hover:bg-gray-800'
                }`}
              >
                {item.name}
              </Link>
            )
          })}
        </nav>
        
        <div className="absolute bottom-4 left-4 right-4">
          <Link
            href="/"
            className="block w-full px-3 py-2 text-center text-sm border border-gray-300 dark:border-gray-700 rounded-md hover:bg-gray-100 dark:hover:bg-gray-800 transition-colors"
          >
            Back to Dashboard
          </Link>
        </div>
      </aside>

      {/* Main content */}
      <main className="flex-1 overflow-auto">{children}</main>
    </div>
  )
}
