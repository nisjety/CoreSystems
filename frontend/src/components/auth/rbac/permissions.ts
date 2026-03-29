/**
 * Permission system for Aquatiq Hub RBAC
 * Based on Sprint 5 RBAC requirements in sprints.md
 */

// Core permissions
export const PERMISSIONS = {
  // Employee permissions
  'employee.read.own': 'Read own employee profile',
  'employee.update.own': 'Update own employee profile',
  
  // HR permissions
  'employee.read.all': 'Read all employee profiles',
  'employee.create': 'Create new employees',
  'employee.update.all': 'Update any employee profile',
  'employee.delete': 'Delete employees',
  'employee.manage.roles': 'Manage employee roles',
  
  // Manager permissions
  'employee.read.team': 'Read team member profiles',
  'employee.update.team': 'Update team member profiles',
  'timebank.approve.team': 'Approve team time entries',
  'procurement.approve.team': 'Approve team procurement requests',
  
  // IT permissions
  'assets.read.all': 'Read all assets',
  'assets.create': 'Create new assets',
  'assets.update': 'Update assets',
  'assets.assign': 'Assign assets to employees',
  'assets.unassign': 'Unassign assets from employees',
  
  // Finance permissions
  'procurement.read.all': 'Read all procurement requests',
  'procurement.approve.all': 'Approve any procurement request',
  'timebank.export': 'Export timebank data',
  'reports.financial': 'Access financial reports',
  
  // Admin permissions
  'admin.full': 'Full administrative access',
  'system.settings': 'Manage system settings',
  'audit.read': 'Read audit logs',
  
  // Time tracking permissions
  'timebank.read.own': 'Read own time entries',
  'timebank.create.own': 'Create own time entries',
  'timebank.update.own': 'Update own time entries',
  'timebank.read.team': 'Read team time entries',
  'timebank.read.all': 'Read all time entries',
  
  // Procurement permissions
  'procurement.read.own': 'Read own procurement requests',
  'procurement.create.own': 'Create own procurement requests',
  'procurement.read.team': 'Read team procurement requests',
  
  // Document permissions
  'documents.read': 'Read documents',
  'documents.create': 'Create documents',
  'documents.update': 'Update documents',
  'documents.delete': 'Delete documents',
  
  // Meeting permissions
  'meetings.read': 'Read meetings',
  'meetings.create': 'Create meetings',
  'meetings.update': 'Update meetings',
  
  // News permissions
  'news.read': 'Read news',
  'news.create': 'Create news',
  'news.update': 'Update news',
  'news.delete': 'Delete news',
} as const

// Role definitions based on Sprint 5 RBAC requirements
export const ROLES = {
  employee: {
    name: 'Employee',
    permissions: [
      'employee.read.own',
      'employee.update.own',
      'timebank.read.own',
      'timebank.create.own',
      'timebank.update.own',
      'procurement.read.own',
      'procurement.create.own',
      'documents.read',
      'meetings.read',
      'news.read',
    ] as Permission[]
  },
  manager: {
    name: 'Manager',
    permissions: [
      // Inherit all employee permissions
      'employee.read.own',
      'employee.update.own',
      'timebank.read.own',
      'timebank.create.own',
      'timebank.update.own',
      'procurement.read.own',
      'procurement.create.own',
      'documents.read',
      'meetings.read',
      'news.read',
      // Additional manager permissions
      'employee.read.team',
      'employee.update.team',
      'timebank.read.team',
      'timebank.approve.team',
      'procurement.read.team',
      'procurement.approve.team',
      'meetings.create',
      'meetings.update',
    ] as Permission[]
  },
  hr: {
    name: 'HR',
    permissions: [
      // All employee management
      'employee.read.all',
      'employee.create',
      'employee.update.all',
      'employee.delete',
      'employee.manage.roles',
      // Time tracking oversight
      'timebank.read.all',
      'timebank.export',
      // Document management
      'documents.read',
      'documents.create',
      'documents.update',
      'documents.delete',
      // News management
      'news.read',
      'news.create',
      'news.update',
      'news.delete',
      // Own profile
      'employee.read.own',
      'employee.update.own',
      'timebank.read.own',
      'timebank.create.own',
      'timebank.update.own',
      'procurement.read.own',
      'procurement.create.own',
      'meetings.read',
    ] as Permission[]
  },
  it: {
    name: 'IT',
    permissions: [
      // Asset management
      'assets.read.all',
      'assets.create',
      'assets.update',
      'assets.assign',
      'assets.unassign',
      // Employee read access for asset assignment
      'employee.read.all',
      // Procurement oversight
      'procurement.read.all',
      // System access
      'documents.read',
      'meetings.read',
      'news.read',
      // Own profile
      'employee.read.own',
      'employee.update.own',
      'timebank.read.own',
      'timebank.create.own',
      'timebank.update.own',
      'procurement.read.own',
      'procurement.create.own',
    ] as Permission[]
  },
  finance: {
    name: 'Finance',
    permissions: [
      // Financial oversight
      'procurement.read.all',
      'procurement.approve.all',
      'timebank.read.all',
      'timebank.export',
      'reports.financial',
      // Employee read for cost center management
      'employee.read.all',
      // Documents
      'documents.read',
      'documents.create',
      'documents.update',
      'meetings.read',
      'news.read',
      // Own profile
      'employee.read.own',
      'employee.update.own',
      'timebank.read.own',
      'timebank.create.own',
      'timebank.update.own',
      'procurement.read.own',
      'procurement.create.own',
    ] as Permission[]
  },
  admin: {
    name: 'Administrator',
    permissions: Object.keys(PERMISSIONS) as Permission[]
  }
} as const

export type Permission = keyof typeof PERMISSIONS
export type RoleName = keyof typeof ROLES

// Helper functions
export function hasPermission(userRoles: string[], requiredPermission: Permission): boolean {
  for (const roleName of userRoles) {
    const role = ROLES[roleName as RoleName]
    if (role && role.permissions.includes(requiredPermission)) {
      return true
    }
  }
  return false
}

export function hasAnyPermission(userRoles: string[], requiredPermissions: Permission[]): boolean {
  return requiredPermissions.some(permission => hasPermission(userRoles, permission))
}

export function hasAllPermissions(userRoles: string[], requiredPermissions: Permission[]): boolean {
  return requiredPermissions.every(permission => hasPermission(userRoles, permission))
}

export function getUserPermissions(userRoles: string[]): Permission[] {
  const permissions = new Set<Permission>()
  
  for (const roleName of userRoles) {
    const role = ROLES[roleName as RoleName]
    if (role) {
      role.permissions.forEach(permission => permissions.add(permission))
    }
  }
  
  return Array.from(permissions)
}

export function isAdmin(userRoles: string[]): boolean {
  return userRoles.includes('admin') || hasPermission(userRoles, 'admin.full')
}

export function isManager(userRoles: string[]): boolean {
  return userRoles.includes('manager') || hasPermission(userRoles, 'timebank.approve.team')
}

export function isHR(userRoles: string[]): boolean {
  return userRoles.includes('hr') || hasPermission(userRoles, 'employee.read.all')
}

export function isIT(userRoles: string[]): boolean {
  return userRoles.includes('it') || hasPermission(userRoles, 'assets.read.all')
}

export function isFinance(userRoles: string[]): boolean {
  return userRoles.includes('finance') || hasPermission(userRoles, 'reports.financial')
}
