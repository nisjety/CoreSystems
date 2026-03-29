import React from 'react'
import { 
  Home,
  Users,
  DollarSign,
  BookOpen,
  Calendar,
  CalendarDays,
  Newspaper,
  MessageSquare,
  FileText,
  GraduationCap,
  Monitor,
  CheckSquare,
  AlertTriangle,
  Bell,
  ShoppingCart,
  BarChart3,
  Settings,
  UserCog,
  FileSearch,
  Building2
} from 'lucide-react'
import type { Permission } from './permissions'

export interface NavigationItem {
  id: string
  label: string
  icon: React.ComponentType<{ className?: string }>
  href: string
  badge?: string | number
  isNew?: boolean
  isComingSoon?: boolean
  hasSubmenu?: boolean
  submenu?: SubmenuItem[]
  requiredPermissions?: Permission[]
  requiredRoles?: string[]
}

export interface SubmenuItem {
  id: string
  label: string
  href: string
  badge?: string | number
  isNew?: boolean
  isComingSoon?: boolean
  requiredPermissions?: Permission[]
  requiredRoles?: string[]
}

// Navigation items with permission requirements
export const NAVIGATION_ITEMS: NavigationItem[] = [
  {
    id: 'dashboard',
    label: 'Dashboard',
    icon: Home,
    href: '/aquatiq',
    // Everyone can access dashboard
  },
  {
    id: 'folk',
    label: 'Folk',
    icon: Users,
    href: '/folk',
    hasSubmenu: true,
    requiredPermissions: ['employee.read.all', 'employee.read.team', 'employee.read.own'],
    submenu: [
      {
        id: 'folk-list',
        label: 'Ansatte',
        href: '/folk',
        requiredPermissions: ['employee.read.all', 'employee.read.team']
      },
      {
        id: 'folk-create',
        label: 'Ny ansatt',
        href: '/folk/new',
        requiredPermissions: ['employee.create']
      },
      {
        id: 'folk-departments',
        label: 'Avdelinger',
        href: '/folk/departments',
        requiredPermissions: ['employee.read.all']
      },
      {
        id: 'folk-roles',
        label: 'Roller',
        href: '/folk/roles',
        requiredPermissions: ['employee.manage.roles'],
        isNew: true
      }
    ]
  },
  {
    id: 'oppgaver',
    label: 'Oppgaver',
    icon: CheckSquare,
    href: '/oppgaver',
    hasSubmenu: true,
    requiredPermissions: ['timebank.read.own', 'timebank.read.team', 'timebank.read.all'],
    submenu: [
      {
        id: 'oppgaver-mine',
        label: 'Mine timer',
        href: '/oppgaver/mine',
        requiredPermissions: ['timebank.read.own']
      },
      {
        id: 'oppgaver-team',
        label: 'Team timer',
        href: '/oppgaver/team',
        requiredPermissions: ['timebank.read.team']
      },
      {
        id: 'oppgaver-approve',
        label: 'Godkjenning',
        href: '/oppgaver/approve',
        requiredPermissions: ['timebank.approve.team'],
        badge: '3'
      },
      {
        id: 'oppgaver-export',
        label: 'Eksporter',
        href: '/oppgaver/export',
        requiredPermissions: ['timebank.export']
      }
    ]
  },
  {
    id: 'utstyr',
    label: 'Utstyr',
    icon: Monitor,
    href: '/utstyr',
    hasSubmenu: true,
    requiredPermissions: ['assets.read.all', 'employee.read.own'],
    submenu: [
      {
        id: 'utstyr-mine',
        label: 'Mitt utstyr',
        href: '/utstyr/mine',
        requiredPermissions: ['employee.read.own']
      },
      {
        id: 'utstyr-all',
        label: 'Alt utstyr',
        href: '/utstyr',
        requiredPermissions: ['assets.read.all']
      },
      {
        id: 'utstyr-assign',
        label: 'Tildeling',
        href: '/utstyr/assign',
        requiredPermissions: ['assets.assign']
      },
      {
        id: 'utstyr-create',
        label: 'Nytt utstyr',
        href: '/utstyr/new',
        requiredPermissions: ['assets.create']
      }
    ]
  },
  {
    id: 'innkjop',
    label: 'Innkjøp',
    icon: ShoppingCart,
    href: '/innkjop',
    hasSubmenu: true,
    requiredPermissions: ['procurement.read.own', 'procurement.read.team', 'procurement.read.all'],
    submenu: [
      {
        id: 'innkjop-mine',
        label: 'Mine forespørsler',
        href: '/innkjop/mine',
        requiredPermissions: ['procurement.read.own']
      },
      {
        id: 'innkjop-create',
        label: 'Ny forespørsel',
        href: '/innkjop/new',
        requiredPermissions: ['procurement.create.own']
      },
      {
        id: 'innkjop-approve',
        label: 'Godkjenning',
        href: '/innkjop/approve',
        requiredPermissions: ['procurement.approve.team', 'procurement.approve.all'],
        badge: '2'
      },
      {
        id: 'innkjop-all',
        label: 'Alle forespørsler',
        href: '/innkjop',
        requiredPermissions: ['procurement.read.all']
      }
    ]
  },
  {
    id: 'lonn',
    label: 'Lønn',
    icon: DollarSign,
    href: '/lonn',
    requiredPermissions: ['timebank.export', 'reports.financial']
  },
  {
    id: 'rapporter',
    label: 'Rapporter',
    icon: BarChart3,
    href: '/rapporter',
    requiredPermissions: ['reports.financial', 'timebank.export'],
    isNew: true
  },
  {
    id: 'handbok',
    label: 'Håndbok',
    icon: BookOpen,
    href: '/handbok'
  },
  {
    id: 'fravar',
    label: 'Fravær',
    icon: Calendar,
    href: '/fravar'
  },
  {
    id: 'moter',
    label: 'Møter',
    icon: CalendarDays,
    href: '/moter'
  },
  {
    id: 'nyheter',
    label: 'Nyheter',
    icon: Newspaper,
    href: '/nyheter'
  },
  {
    id: 'meldinger',
    label: 'Meldinger',
    icon: MessageSquare,
    href: '/meldinger'
  },
  {
    id: 'dokumenter',
    label: 'Dokumenter',
    icon: FileText,
    href: '/dokumenter'
  },
  {
    id: 'kontraktadministrasjon',
    label: 'Kontrakt Admin',
    icon: Building2,
    href: '/kontrakt-admin',
    isNew: true
  },
  {
    id: 'kompetanse',
    label: 'Kompetanse',
    icon: GraduationCap,
    href: '/kompetanse'
  },
  {
    id: 'avvik',
    label: 'Avvik',
    icon: AlertTriangle,
    href: '/avvik'
  },
  {
    id: 'varslerportal',
    label: 'Varsler',
    icon: Bell,
    href: '/varslerportal'
  },
  
  // Admin section
  {
    id: 'brukeradmin',
    label: 'Brukeradmin',
    icon: UserCog,
    href: '/brukeradmin',
    requiredPermissions: ['employee.manage.roles', 'admin.full'],
    requiredRoles: ['admin', 'hr']
  },
  {
    id: 'instillinger',
    label: 'Innstillinger',
    icon: Settings,
    href: '/instillinger',
    requiredPermissions: ['system.settings', 'admin.full'],
    requiredRoles: ['admin']
  },
  {
    id: 'audit',
    label: 'Audit Log',
    icon: FileSearch,
    href: '/audit',
    requiredPermissions: ['audit.read'],
    requiredRoles: ['admin']
  }
]

// Helper function to check if user can access navigation item
export function hasAccessToNavItem(
  item: NavigationItem,
  userRoles: string[],
  hasPermission: (permission: Permission) => boolean,
  hasAnyPermission: (permissions: Permission[]) => boolean
): boolean {
  // Check role requirements
  if (item.requiredRoles && item.requiredRoles.length > 0) {
    const hasRequiredRole = item.requiredRoles.some(role => 
      userRoles.includes(role.toLowerCase())
    )
    if (!hasRequiredRole) return false
  }

  // Check permission requirements
  if (item.requiredPermissions && item.requiredPermissions.length > 0) {
    return hasAnyPermission(item.requiredPermissions)
  }

  // If no specific requirements, allow access
  return true
}

// Helper function to check submenu item access
export function hasAccessToSubmenuItem(
  item: SubmenuItem,
  userRoles: string[],
  hasPermission: (permission: Permission) => boolean,
  hasAnyPermission: (permissions: Permission[]) => boolean
): boolean {
  // Check role requirements
  if (item.requiredRoles && item.requiredRoles.length > 0) {
    const hasRequiredRole = item.requiredRoles.some(role => 
      userRoles.includes(role.toLowerCase())
    )
    if (!hasRequiredRole) return false
  }

  // Check permission requirements
  if (item.requiredPermissions && item.requiredPermissions.length > 0) {
    return hasAnyPermission(item.requiredPermissions)
  }

  // If no specific requirements, allow access
  return true
}

// Get visible navigation items for current user
export function getVisibleNavigation(
  userRoles: string[],
  hasPermission: (permission: Permission) => boolean,
  hasAnyPermission: (permissions: Permission[]) => boolean
): NavigationItem[] {
  return NAVIGATION_ITEMS.filter(item => 
    hasAccessToNavItem(item, userRoles, hasPermission, hasAnyPermission)
  )
}

// Get visible submenu items for current user
export function getVisibleSubmenu(
  submenu: SubmenuItem[] = [],
  userRoles: string[],
  hasPermission: (permission: Permission) => boolean,
  hasAnyPermission: (permissions: Permission[]) => boolean
): SubmenuItem[] {
  return submenu.filter(item => 
    hasAccessToSubmenuItem(item, userRoles, hasPermission, hasAnyPermission)
  )
}

// Get navigation item by ID
export function getNavigationItem(id: string): NavigationItem | undefined {
  return NAVIGATION_ITEMS.find(item => item.id === id)
}

// Get submenu item by ID
export function getSubmenuItem(navId: string, submenuId: string): SubmenuItem | undefined {
  const navItem = getNavigationItem(navId)
  if (!navItem?.submenu) return undefined
  return navItem.submenu.find(item => item.id === submenuId)
}
