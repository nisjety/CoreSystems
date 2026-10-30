import type { LucideIcon } from 'lucide-react';
import { Home, Sparkles, Layers, Video, Shield, FileText, Truck, Plug, FileSignature } from 'lucide-react';

export type SharedNavItem = {
  id: string;
  labelKey: string;
  defaultLabel: string;
  href: string;
  icon: LucideIcon;
  badge?: number;
};

export const sharedNavItems: SharedNavItem[] = [
  {
    id: 'dashboard',
    labelKey: 'dashboard',
    defaultLabel: 'Dashboard',
    href: '/',
    icon: Home,
  },
  {
    id: 'ai-studio',
    labelKey: 'aiStudio',
    defaultLabel: 'AI Studio',
    href: '/ai-studio',
    icon: Sparkles,
  },
  {
    id: 'notes',
    labelKey: 'notes',
    defaultLabel: 'Notes',
    href: '/notes',
    icon: FileText,
  },
  {
    id: 'file-manager',
    labelKey: 'fileManager',
    defaultLabel: 'File Manager',
    href: '/file-manager',
    icon: Layers,
  },
  {
    id: 'erp-connector',
    labelKey: 'integrations',
    defaultLabel: 'Integrations',
    href: '/erp-connector',
    icon: Plug,
  },
  {
    id: 'contract-management',
    labelKey: 'contractManagement',
    defaultLabel: 'Contract Management',
    href: '/contract-admin',
    icon: FileSignature,
  },
  {
    id: 'signage',
    labelKey: 'signage',
    defaultLabel: 'Signage',
    href: '/signage',
    icon: Video,
  },
  {
    id: 'bx-dashboard',
    labelKey: 'bxDashboard',
    defaultLabel: 'BX Logistics',
    href: '/bx-dashboard',
    icon: Truck,
  },
  {
    id: 'risk-manager',
    labelKey: 'riskManager',
    defaultLabel: 'Risk Manager',
    href: '/risk-manager',
    icon: Shield,
  },
];
