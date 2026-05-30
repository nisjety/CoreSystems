'use client'

import { useEffect } from 'react'
import { adminService, type OrgWithDetails } from '@/components/admin/services/admin-service'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { Badge } from '@/components/ui/badge'
import type { OrgPlan } from '@/lib/services/org-service'
import { useOrgManagementState } from './use-org-management-state'

export function OrgManagement() {
  const [state, dispatch] = useOrgManagementState()
  const { orgs, loading, error, searchQuery, selectedOrg } = state

  useEffect(() => {
    loadOrganizations()
  }, [])

  const loadOrganizations = async () => {
    try {
      dispatch({ type: 'FETCH_START' })
      const data = await adminService.getAllOrgsWithDetails()
      dispatch({ type: 'FETCH_SUCCESS', payload: data })
    } catch (err: any) {
      dispatch({ type: 'FETCH_ERROR', payload: err.message || 'Failed to load organizations' })
    }
  }

  const filteredOrgs = orgs.filter(
    org =>
      org.name.toLowerCase().includes(searchQuery.toLowerCase()) ||
      org.slug.toLowerCase().includes(searchQuery.toLowerCase())
  )

  const handleSuspend = async (orgId: string) => {
    try {
      await adminService.suspendOrganization(orgId)
      await loadOrganizations()
    } catch (err: any) {
      alert(err.message || 'Failed to suspend organization')
    }
  }

  const handleActivate = async (orgId: string) => {
    try {
      await adminService.activateOrganization(orgId)
      await loadOrganizations()
    } catch (err: any) {
      alert(err.message || 'Failed to activate organization')
    }
  }

  const handleChangePlan = async (orgId: string, plan: OrgPlan) => {
    try {
      await adminService.updateOrgPlan(orgId, plan)
      await loadOrganizations()
    } catch (err: any) {
      alert(err.message || 'Failed to update plan')
    }
  }

  if (loading) {
    return <div className="p-8">Loading organizations...</div>
  }

  if (error) {
    return (
      <div className="p-8">
        <div className="p-4 text-red-600 bg-red-50 rounded-md">{error}</div>
      </div>
    )
  }

  return (
    <div className="space-y-6">
      <div className="flex justify-between items-center">
        <div>
          <h2 className="text-3xl font-bold">Organization Management</h2>
          <p className="text-muted-foreground">
            Manage all organizations and their settings
          </p>
        </div>
      </div>

      <div className="flex gap-4">
        <Input
          placeholder="Search organizations..."
          value={searchQuery}
          onChange={(e) => dispatch({ type: 'SET_SEARCH', payload: e.target.value })}
          className="max-w-sm"
        />
        <Button onClick={loadOrganizations}>Refresh</Button>
      </div>

      <div className="grid gap-4">
        {filteredOrgs.map((org) => (
          <Card key={org.id}>
            <CardHeader>
              <div className="flex justify-between items-start">
                <div>
                  <CardTitle>{org.name}</CardTitle>
                  <CardDescription>Slug: {org.slug}</CardDescription>
                </div>
                <div className="flex gap-2">
                  <Badge variant={org.status === 'active' ? 'default' : 'destructive'}>
                    {org.status}
                  </Badge>
                  <Badge variant="outline">{org.plan.toUpperCase()}</Badge>
                </div>
              </div>
            </CardHeader>
            <CardContent>
              <div className="grid gap-4 md:grid-cols-2">
                <div>
                  <h4 className="font-semibold mb-2">Details</h4>
                  <div className="space-y-1 text-sm">
                    <p>Members: {org.memberCount}</p>
                    <p>Created: {new Date(org.createdAt).toLocaleDateString()}</p>
                  </div>
                </div>

                <div>
                  <h4 className="font-semibold mb-2">Quota Usage</h4>
                  <div className="space-y-1 text-sm">
                    <p>API Calls: {org.quotaUsage.apiCalls.toLocaleString()}</p>
                    <p>Users: {org.quotaUsage.users}</p>
                    <p>Storage: {org.quotaUsage.storage} MB</p>
                  </div>
                </div>

                <div>
                  <h4 className="font-semibold mb-2">Billing</h4>
                  <div className="space-y-1 text-sm">
                    <p>Status: {org.billing.subscriptionStatus}</p>
                    {org.billing.stripeCustomerId && (
                      <p className="text-xs text-muted-foreground">
                        Stripe: {org.billing.stripeCustomerId}
                      </p>
                    )}
                  </div>
                </div>

                <div>
                  <h4 className="font-semibold mb-2">Actions</h4>
                  <div className="flex gap-2 flex-wrap">
                    <select
                      value={org.plan}
                      onChange={(e) =>
                        handleChangePlan(org.id, e.target.value as OrgPlan)
                      }
                      className="text-xs px-2 py-1 border rounded"
                    >
                      <option value="free">Free</option>
                      <option value="trial">Trial</option>
                      <option value="hobby">Essential</option>
                      <option value="standard">Advanced</option>
                      <option value="pro">Expert</option>
                      <option value="enterprise">Custom</option>
                    </select>
                    
                    {org.status === 'active' ? (
                      <Button
                        variant="destructive"
                        size="sm"
                        onClick={() => handleSuspend(org.id)}
                      >
                        Suspend
                      </Button>
                    ) : (
                      <Button
                        variant="default"
                        size="sm"
                        onClick={() => handleActivate(org.id)}
                      >
                        Activate
                      </Button>
                    )}

                    <Button
                      variant="outline"
                      size="sm"
                      onClick={() => dispatch({ type: 'SET_SELECTED_ORG', payload: org })}
                    >
                      View Details
                    </Button>
                  </div>
                </div>
              </div>
            </CardContent>
          </Card>
        ))}
      </div>

      {filteredOrgs.length === 0 && (
        <div className="text-center py-12 text-muted-foreground">
          No organizations found
        </div>
      )}
    </div>
  )
}
