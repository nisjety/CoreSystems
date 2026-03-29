import { requireAuth } from '../../components/auth/lib/auth-server'
import { redirect } from 'next/navigation'
import { AdminLayoutClient } from '../../components/admin/AdminLayoutClient'

export default async function AdminLayout({
  children,
}: {
  children: React.ReactNode
}) {
  // Server-side auth check
  const session = await requireAuth()
  const { user } = session

  // TODO: Add proper admin role check
  // For now, any authenticated user can access
  // In production: if (user.role !== 'admin') redirect('/dashboard')

  return <AdminLayoutClient user={user}>{children}</AdminLayoutClient>
}
