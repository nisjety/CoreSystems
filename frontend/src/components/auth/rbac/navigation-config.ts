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
  Settings,
  BarChart3,
  Clock,
  ShoppingCart,
  UserCog,
  Shield
} from 'lucide-react'
import type { Permission } from './permissions'

export interface NavigationItem {
  id: string
  label: string
  icon: React.ComponentType<{ className?: string }>
  href: string
  description?: string
  
  // Permission requirements
  permission?: Permission
  permissions?: Permission[]
  requireAll?: boolean
  roles?: string[]
  
  // UI properties
  badge?: string | number
  isNew?: boolean
  isComingSoon?: boolean
  
  // Grouping
  group?: 'core' | 'hr' | 'operations' | 'admin'
  order?: number
  
  // Submenu
  hasSubmenu?: boolean
  submenu?: NavigationSubItem[]
}

export interface NavigationSubItem {
  id: string
  label: string
  href: string
  permission?: Permission
  permissions?: Permission[]
  roles?: string[]
  isNew?: boolean
  isComingSoon?: boolean
}

// Navigation configuration based on RBAC requirements
export const NAVIGATION_CONFIG: NavigationItem[] = [
  // Core items - available to all users
  {
    id: 'dashboard',
    label: 'Dashboard',
    icon: Home,
    href: '/aquatiq',
    description: 'Overview and quick access',
    group: 'core',
    order: 1,
  },
  
  // HR & People Management
  {
    id: 'folk',
    label: 'Folk',
    icon: Users,
    href: '/folk',
    description: 'Employee management',
    group: 'hr',
    order: 2,
    permissions: ['employee.read.all', 'employee.read.team', 'employee.read.own'],
    hasSubmenu: true,
    submenu: [
      {
        id: 'folk-employees',
        label: 'Ansatte',
        href: '/folk',
        permissions: ['employee.read.all', 'employee.read.team'],
      },
      {
        id: 'folk-profile',
        label: 'Min profil',
        href: '/profile',
        permission: 'employee.read.own',
      },
      {
        id: 'folk-teams',
        label: 'Team',
        href: '/folk/teams',
        permissions: ['employee.read.team', 'employee.read.all'],
      },
      {
        id: 'folk-roles',
        label: 'Roller',
        href: '/folk/roles',
        permission: 'employee.manage.roles',
      }
    ]
  },

  // Time Management
  {
    id: 'oppgaver',
    label: 'Oppgaver & Timer',
    icon: CheckSquare,
    href: '/oppgaver',
    description: 'Time tracking and task management',
    group: 'operations',
    order: 3,
    permission: 'timebank.read.own',
    hasSubmenu: true,
    submenu: [
      {
        id: 'oppgaver-mine',
        label: 'Mine timer',
        href: '/oppgaver',
        permission: 'timebank.read.own',
      },
      {
        id: 'oppgaver-team',
        label: 'Team timer',
        href: '/oppgaver/team',
        permission: 'timebank.read.team',
      },
      {
        id: 'oppgaver-approve',
        label: 'Godkjenn timer',
        href: '/oppgaver/approve',
        permission: 'timebank.approve.team',
      },
      {
        id: 'oppgaver-export',
        label: 'Eksporter til lønn',
        href: '/oppgaver/export',
        permission: 'timebank.export',
      }
    ]
  },

  // Payroll - Finance focused
  {
    id: 'lonn',
    label: 'Lønn',
    icon: DollarSign,
    href: '/lonn',
    description: 'Payroll and financial reports',
    group: 'operations',
    order: 4,
    permissions: ['timebank.export', 'reports.financial'],
  },

  // Assets & Equipment - IT focused
  {
    id: 'utstyr',
    label: 'Utstyr',
    icon: Monitor,
    href: '/utstyr',
    description: 'Equipment and asset management',
    group: 'operations',
    order: 5,
    permissions: ['assets.read.all', 'employee.read.own'], // All can see their own equipment
    hasSubmenu: true,
    submenu: [
      {
        id: 'utstyr-mine',
        label: 'Mitt utstyr',
        href: '/utstyr',
        permission: 'employee.read.own',
      },
      {
        id: 'utstyr-all',
        label: 'Alle eiendeler',
        href: '/utstyr/all',
        permission: 'assets.read.all',
      },
      {
        id: 'utstyr-assign',
        label: 'Tildel utstyr',
        href: '/utstyr/assign',
        permission: 'assets.assign',
      }
    ]
  },

  // Procurement
  {
    id: 'innkjop',
    label: 'Innkjøp',
    icon: ShoppingCart,
    href: '/innkjop',
    description: 'Procurement and purchasing',
    group: 'operations',
    order: 6,
    permission: 'procurement.read.own',
    isNew: true,
    hasSubmenu: true,
    submenu: [
      {
        id: 'innkjop-mine',
        label: 'Mine rekvisisjoner',
        href: '/innkjop',
        permission: 'procurement.read.own',
      },
      {
        id: 'innkjop-new',
        label: 'Ny rekvisisjon',
        href: '/innkjop/new',
        permission: 'procurement.create.own',
      },
      {
        id: 'innkjop-approve',
        label: 'Godkjenn innkjøp',
        href: '/innkjop/approve',
        permissions: ['procurement.approve.team', 'procurement.approve.all'],
      },
      {
        id: 'innkjop-all',
        label: 'Alle rekvisisjoner',
        href: '/innkjop/all',
        permissions: ['procurement.read.all', 'procurement.read.team'],
      }
    ]
  },

  // Absence Management
  {
    id: 'fravar',
    label: 'Fravær',
    icon: Calendar,
    href: '/fravar',
    description: 'Absence and leave management',
    group: 'hr',
    order: 7,
    permission: 'employee.read.own',
    isComingSoon: true,
  },

  // Communication & Collaboration
  {
    id: 'moter',
    label: 'Møter',
    icon: CalendarDays,
    href: '/moter',
    description: 'Meeting management',
    group: 'core',
    order: 8,
    permission: 'meetings.read',
  },

  {
    id: 'meldinger',
    label: 'Meldinger',
    icon: MessageSquare,
    href: '/meldinger',
    description: 'Internal messaging',
    group: 'core',
    order: 9,
    permission: 'employee.read.own',
    badge: 3, // Example badge
  },

  // Knowledge & Documentation
  {
    id: 'handbok',
    label: 'Håndbok',
    icon: BookOpen,
    href: '/handbok',
    description: 'Company handbook and policies',
    group: 'core',
    order: 10,
    permission: 'documents.read',
  },

  {
    id: 'dokumenter',
    label: 'Dokumenter',
    icon: FileText,
    href: '/dokumenter',
    description: 'Document management',
    group: 'core',
    order: 11,
    permission: 'documents.read',
  },

  {
    id: 'nyheter',
    label: 'Nyheter',
    icon: Newspaper,
    href: '/nyheter',
    description: 'Company news and announcements',
    group: 'core',
    order: 12,
    permission: 'news.read',
  },

  // Learning & Development
  {
    id: 'kompetanse',
    label: 'Kompetanse',
    icon: GraduationCap,
    href: '/kompetanse',
    description: 'Learning and development',
    group: 'hr',
    order: 13,
    permission: 'employee.read.own',
    isComingSoon: true,
  },

  // Quality & Compliance
  {
    id: 'avvik',
    label: 'Avvik',
    icon: AlertTriangle,
    href: '/avvik',
    description: 'Incident and deviation reporting',
    group: 'operations',
    order: 14,
    permission: 'employee.read.own',
    isComingSoon: true,
  },

  // Admin & System Management
  {
    id: 'rapporter',
    label: 'Rapporter',
    icon: BarChart3,
    href: '/rapporter',
    description: 'Analytics and reporting',
    group: 'admin',
    order: 15,
    permissions: ['reports.financial', 'timebank.export', 'admin.full'],
  },

  {
    id: 'instillinger',
    label: 'Innstillinger',
    icon: Settings,
    href: '/instillinger',
    description: 'System settings',
    group: 'admin',
    order: 16,
    permissions: ['system.settings', 'admin.full'],
  },

  {
    id: 'brukeradmin',
    label: 'Brukeradministrasjon',
    icon: UserCog,
    href: '/brukeradmin',
    description: 'User and role management',
    group: 'admin',
    order: 17,
    permissions: ['employee.manage.roles', 'admin.full'],
  },

  {
    id: 'audit',
    label: 'Revisjonslogg',
    icon: Shield,
    href: '/audit',
    description: 'Audit logs and security',
    group: 'admin',
    order: 18,
    permission: 'audit.read',
  },
]

// Helper functions for navigation filtering
export function getVisibleNavigation(
  userRoles: string[],
  hasPermission: (permission: Permission) => boolean,
  hasAnyPermission: (permissions: Permission[]) => boolean
): NavigationItem[] {
  return NAVIGATION_CONFIG.filter(item => {
    // Check role-based access
    if (item.roles && item.roles.length > 0) {
      const hasRequiredRole = item.roles.some(role => 
        userRoles.map(r => r.toLowerCase()).includes(role.toLowerCase())
      )
      if (!hasRequiredRole) return false
    }

    // Check permission-based access
    if (item.permission && !hasPermission(item.permission)) {
      return false
    }

    if (item.permissions && item.permissions.length > 0) {
      if (item.requireAll) {
        // Must have ALL permissions
        const hasAllPermissions = item.permissions.every(p => hasPermission(p))
        if (!hasAllPermissions) return false
      } else {
        // Must have ANY permission
        if (!hasAnyPermission(item.permissions)) return false
      }
    }

    return true
  }).sort((a, b) => (a.order || 999) - (b.order || 999))
}

export function getVisibleSubmenu(
  submenu: NavigationSubItem[] | undefined,
  userRoles: string[],
  hasPermission: (permission: Permission) => boolean,
  hasAnyPermission: (permissions: Permission[]) => boolean
): NavigationSubItem[] {
  if (!submenu) return []

  return submenu.filter(item => {
    // Check role-based access
    if (item.roles && item.roles.length > 0) {
      const hasRequiredRole = item.roles.some(role => 
        userRoles.map(r => r.toLowerCase()).includes(role.toLowerCase())
      )
      if (!hasRequiredRole) return false
    }

    // Check permission-based access
    if (item.permission && !hasPermission(item.permission)) {
      return false
    }

    if (item.permissions && item.permissions.length > 0) {
      if (!hasAnyPermission(item.permissions)) return false
    }

    return true
  })
}

// Group navigation items by category
export function getNavigationGroups(
  navigation: NavigationItem[]
): Record<string, NavigationItem[]> {
  const groups: Record<string, NavigationItem[]> = {
    core: [],
    hr: [],
    operations: [],
    admin: [],
  }

  navigation.forEach(item => {
    const group = item.group || 'core'
    if (!groups[group]) {
      groups[group] = []
    }
    groups[group].push(item)
  })

  return groups
}
