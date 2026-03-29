'use client'

import { useEffect, useState } from 'react'
import { adminService } from '@/components/admin/services/admin-service'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'

export default function BillingPage() {
  const [stats, setStats] =useState<any>(null)
  const [loading, setLoading] = useState(true)

  useEffect(() => {
    loadStats()
  }, [])

  const loadStats = async () => {
    try {
      const data = await adminService.getDashboardStats()
      setStats(data)
    } catch (err) {
      console.error(err)
    } finally {
      setLoading(false)
    }
  }

  if (loading) return <div className="p-8">Loading...</div>

  return (
    <div className="container mx-auto p-8 space-y-6">
      <div>
        <h2 className="text-3xl font-bold">Billing Overview</h2>
        <p className="text-muted-foreground">
          System-wide billing and subscription management
        </p>
      </div>

      <Card>
        <CardHeader>
          <CardTitle>Billing Statistics</CardTitle>
          <CardDescription>Overview of subscription and billing data</CardDescription>
        </CardHeader>
        <CardContent>
          <div className="text-center py-12 text-muted-foreground">
            Billing management interface - coming soon
          </div>
        </CardContent>
      </Card>
    </div>
  )
}
