'use client';

import React, { useState, useRef } from 'react';
import { cn } from '../utils';
import { SimpleTooltip } from '../ui/simple-tooltip';
import { useCompatibleLanguage } from '@/components/shared/contexts/GlobalLanguageContext';
import { ChevronDown, ChevronRight } from 'lucide-react';
import { sharedNavItems } from '../config/nav-items';

// Types
interface NavigationItem {
  id: string;
  label: string;
  icon: React.ComponentType<{ className?: string }>;
  href: string;
  badge?: number;
  children?: NavigationItem[];
}

interface NavigationProps {
  activeItem?: string;
  onNavigate: (item: NavigationItem) => void;
  className?: string;
  isCollapsed?: boolean;
  expandedFloatingMenu?: string | null;
  onFloatingMenuChange?: (itemId: string | null) => void;
}

// i18n translations
const translations = {
  en: {
    dashboard: 'Dashboard',
    project: 'Project', 
    revenue: 'Revenue',
    analitics: 'Analytics',
    aquatiq: 'Aquatiq',
    management: 'Management',
    timeManagement: 'Time Management',
    timeRegister: 'Time Register',
    scheduled: 'Scheduled',
    timesheets: 'Timesheets',
    timeTracking: 'Time Tracking',
    hrManagement: 'HR Management',
    employees: 'Employees',
    recruitment: 'Recruitment',
    performance: 'Performance',
    payroll: 'Payroll',
    equipmentAssets: 'Equipment Assets',
    computers: 'Computers',
    mobileDevices: 'Mobile Devices',
    officeEquipment: 'Office Equipment',
    procurement: 'Procurement',
    purchaseRequests: 'Purchase Requests',
    vendors: 'Vendors',
    purchaseOrders: 'Purchase Orders',
    messages: 'Messages',
    notifications: 'Notifications',
    calendar: 'Calendar',
    aiStudio: 'AI Studio',
    duplicateFinder: 'Duplicate Finder',
    signage: 'Signage',
    riskManager: 'Risk Manager'
  },
  no: {
    dashboard: 'Dashbord',
    project: 'Prosjekt',
    revenue: 'Inntekt', 
    analitics: 'Analytikk',
    aquatiq: 'Aquatiq',
    management: 'Administrasjon',
    timeManagement: 'Tidsadministrasjon',
    timeRegister: 'Tidsregistrering',
    scheduled: 'Planlagt',
    timesheets: 'Timelister',
    timeTracking: 'Tidssporing',
    hrManagement: 'HR-administrasjon',
    employees: 'Ansatte',
    recruitment: 'Rekruttering', 
    performance: 'Ytelse',
    payroll: 'Lønn',
    equipmentAssets: 'Utstyr og eiendeler',
    computers: 'Datamaskiner',
    mobileDevices: 'Mobile enheter',
    officeEquipment: 'Kontorutstyr',
    procurement: 'Innkjøp',
    purchaseRequests: 'Innkjøpsforespørsler',
    vendors: 'Leverandører',
    purchaseOrders: 'Bestillinger',
    messages: 'Meldinger',
    notifications: 'Varsler',
    calendar: 'Kalender',
    aiStudio: 'AI Studio',
    duplicateFinder: 'Duplikatfinner',
    signage: 'Skiltvisning',
    riskManager: 'Risikostyring'
  }
};

export function Navigation({ 
  activeItem, 
  onNavigate, 
  className,
  isCollapsed = false,
  expandedFloatingMenu,
  onFloatingMenuChange
}: NavigationProps) {
  const [expandedItems, setExpandedItems] = useState<Set<string>>(new Set(['dashboard']));
  const menuRefs = useRef<{ [key: string]: HTMLButtonElement | null }>({});
  
  // Use global language context
  const { sidebarLocale } = useCompatibleLanguage();
  const language = sidebarLocale;

  const t = (key: string) => {
    return translations[language][key as keyof typeof translations[typeof language]] || key;
  };

  // Navigation items following Miller's Law - grouped logically
  const navigationItems: NavigationItem[] = sharedNavItems.map((item) => ({
    id: item.id,
    label: t(item.labelKey) || item.defaultLabel,
    icon: item.icon,
    href: item.href,
    badge: item.badge,
    children: undefined,
  }));

  const toggleExpand = (itemId: string) => {
    if (isCollapsed) return; // Don't expand in collapsed mode
    
    const newExpanded = new Set(expandedItems);
    if (newExpanded.has(itemId)) {
      newExpanded.delete(itemId);
    } else {
      newExpanded.add(itemId);
    }
    setExpandedItems(newExpanded);
  };

  const handleItemClick = (item: NavigationItem) => {
    if (item.children && item.children.length > 0) {
      if (isCollapsed) {
        // Handle floating menu for collapsed sidebar
        const newExpandedId = expandedFloatingMenu === item.id ? null : item.id;
        onFloatingMenuChange?.(newExpandedId);
      } else {
        // Handle regular expansion for non-collapsed sidebar
        toggleExpand(item.id);
      }
    } else {
      onNavigate(item);
      // Close floating menu when navigating to a leaf item
      if (isCollapsed) {
        onFloatingMenuChange?.(null);
      }
    }
  };

  // Enhanced floating menu component for collapsed sidebar
  const renderFloatingMenu = () => {
    if (!isCollapsed || !expandedFloatingMenu) return null;

    // Find the expanded item in the navigation tree
    const findItemById = (items: NavigationItem[], id: string): NavigationItem | null => {
      for (const item of items) {
        if (item.id === id) return item;
        if (item.children) {
          const found = findItemById(item.children, id);
          if (found) return found;
        }
      }
      return null;
    };

    const expandedItem = findItemById(navigationItems, expandedFloatingMenu);
    if (!expandedItem || !expandedItem.children) return null;

    // Get the root parent item to maintain consistent positioning
    const getRootParent = (itemId: string): string => {
      // Check if this is a top-level item
      if (navigationItems.some(item => item.id === itemId)) {
        return itemId;
      }
      
      // Find which top-level item this belongs to
      for (const topItem of navigationItems) {
        if (topItem.children) {
          const findInChildren = (children: NavigationItem[]): boolean => {
            for (const child of children) {
              if (child.id === itemId) return true;
              if (child.children && findInChildren(child.children)) return true;
            }
            return false;
          };
          
          if (findInChildren(topItem.children)) {
            return topItem.id;
          }
        }
      }
      
      return itemId; // fallback
    };

    const rootParent = getRootParent(expandedFloatingMenu);
    
    // Always position relative to the root parent button for consistent placement
    const getConsistentPosition = () => {
      const rootButton = menuRefs.current[rootParent];
      if (rootButton) {
        const rect = rootButton.getBoundingClientRect();
        return rect.top;
      }
      
      // Fallback positioning based on root parent
      const parentIndex = navigationItems.findIndex(item => item.id === rootParent);
      return 120 + (parentIndex * 48); // Calculated position
    };

    const topPosition = getConsistentPosition();

    return (
      <div 
        className="fixed z-50 bg-[#151F6C] rounded-lg shadow-xl border border-white/10 py-1.5 min-w-40 max-w-[200px]"
        style={{ 
          left: '100px', // Consistent positioning next to sidebar
          top: `${topPosition}px`,
          maxHeight: 'calc(100vh - 100px)',
          overflowY: 'auto',
          boxShadow: '0 10px 25px rgba(0, 0, 0, 0.1), 0 4px 6px rgba(0, 0, 0, 0.05)'
        }}
      >
        {/* Menu Items - Clean design matching the image */}
        <div className="py-1">
          {expandedItem.children.map((childItem, index) => (
            <button
              key={childItem.id}
              onClick={() => {
                if (childItem.children && childItem.children.length > 0) {
                  // If this child also has children, expand it (keeping same position)
                  onFloatingMenuChange?.(childItem.id);
                } else {
                  // Navigate to leaf item and close menu
                  onNavigate(childItem);
                  onFloatingMenuChange?.(null);
                }
              }}
              className={cn(
                'w-full flex items-center gap-3 px-3 py-2 text-left hover:bg-white/10 transition-colors text-sm',
                activeItem === childItem.id 
                  ? 'bg-white/20 text-white font-medium' 
                  : 'text-white/80 hover:text-white',
                index === 0 && 'rounded-t-lg',
                index === expandedItem.children!.length - 1 && 'rounded-b-lg'
              )}
            >
              <span className="text-sm font-medium">{childItem.label}</span>
              {/* Show chevron if this item has children */}
              {childItem.children && childItem.children.length > 0 && (
                <ChevronRight className="w-3 h-3 ml-auto text-white/60" />
              )}
            </button>
          ))}
        </div>
      </div>
    );
  };

  // Recursive rendering following Law of Proximity and Visual Hierarchy
  const renderNavigationItem = (
    item: NavigationItem, 
    level = 0, 
    isLast = false, 
    parentConnectors: boolean[] = []
  ) => {
    const isActive = activeItem === item.id;
    const isExpanded = expandedItems.has(item.id);
    const hasChildren = item.children && item.children.length > 0;
    const isFloatingMenuOpen = isCollapsed && expandedFloatingMenu === item.id;

    const navigationButton = (
      <button
        ref={(el) => { menuRefs.current[item.id] = el; }}
        onClick={() => handleItemClick(item)}
        className={cn(
          // Base styles following Fitts's Law - adequate touch targets
          'w-full flex items-center gap-2.5 px-2.5 py-2.5 rounded-xl text-left transition-all duration-200 group relative overflow-hidden',
          
          // Indentation for hierarchy - but not in collapsed mode
          !isCollapsed && level === 1 && 'ml-6',
          !isCollapsed && level === 2 && 'ml-10',
          !isCollapsed && level === 3 && 'ml-14',
          
          // Active state - Von Restorff Effect
          isActive
            ? 'bg-white/20 text-white font-medium shadow-sm'
            : 'text-white/80 hover:bg-white/10 hover:text-white',
          
          // Collapsed mode centering
          isCollapsed && 'justify-center px-2.5',
          
          // Highlight if floating menu is open for this item
          isFloatingMenuOpen && 'bg-white/20 text-white'
        )}
        style={{
          marginLeft: level > 0 && !isCollapsed ? `${20 + level * 20}px` : undefined,
          minHeight: '38px' // Ensure adequate touch target
        }}
      >
        {/* Icon */}
        <span className={cn(
          'shrink-0 transition-colors duration-200 relative',
          level > 0 && !isCollapsed ? 'w-3.5 h-3.5' : 'w-4 h-4',
          isActive ? 'text-white' : 'text-white/60 group-hover:text-white'
        )}>
          <item.icon className={level > 0 && !isCollapsed ? 'w-3.5 h-3.5' : 'w-4 h-4'} />
          {/* Small indicator for items with children when collapsed */}
          {isCollapsed && hasChildren && (
            <div className="absolute -bottom-0.5 -right-0.5 w-1.5 h-1.5 bg-white/60 rounded-full"></div>
          )}
        </span>
        
        {/* Label and extras - only show when not collapsed */}
        {!isCollapsed && (
          <>
            <span className={cn(
              'flex-1 font-medium transition-all duration-200 truncate',
              level > 0 ? 'text-xs' : 'text-sm'
            )}>
              {item.label}
            </span>
            
            {/* Badge for notifications/messages */}
            {item.badge && item.badge > 0 && (
              <span className="px-1.5 py-0.5 bg-red-500 text-white text-xs font-medium rounded-full min-w-[18px] h-4 flex items-center justify-center">
                {item.badge > 99 ? '99+' : item.badge}
              </span>
            )}
            
            {/* Expand/collapse indicator */}
            {hasChildren && (
              <span className="text-white/60 transition-transform duration-200">
                {isExpanded ? (
                  <ChevronDown className="w-3.5 h-3.5" />
                ) : (
                  <ChevronRight className="w-3.5 h-3.5" />
                )}
              </span>
            )}
          </>
        )}
      </button>
    );

    return (
      <div key={item.id} className="relative">
        {/* Visual connectors for hierarchy - Law of Uniform Connectedness */}
        {level > 0 && !isCollapsed && (
          <>
            {parentConnectors.map((showLine, index) => (
              showLine && (
                <div
                  key={index}
                  className="absolute w-px bg-white/20 transition-opacity duration-200"
                  style={{
                    left: `${16 + index * 20}px`,
                    top: '-10px',
                    height: '38px'
                  }}
                />
              )
            ))}
            
            {/* SVG connector for smooth curves */}
            <svg
              className="absolute pointer-events-none transition-opacity duration-200"
              style={{
                left: `${16 + (level - 1) * 20}px`,
                top: '-10px',
                width: '20px',
                height: isLast ? '28px' : '38px'
              }}
            >
              <line
                x1="0"
                y1="0"
                x2="0"
                y2={isLast ? "18" : "28"}
                stroke="#d1d5db"
                strokeWidth="1"
              />
              <path
                d={`M 0 ${isLast ? "18" : "28"} Q 0 ${isLast ? "22" : "32"} 6 ${isLast ? "22" : "32"} L 16 ${isLast ? "22" : "32"}`}
                fill="none"
                stroke="#d1d5db"
                strokeWidth="1"
              />
              {!isLast && (
                <line
                  x1="0"
                  y1="32"
                  x2="0"
                  y2="38"
                  stroke="#d1d5db"
                  strokeWidth="1"
                />
              )}
            </svg>
          </>
        )}

        {isCollapsed ? (
          <SimpleTooltip 
            content={item.label} 
            placement="right" 
            delay={200}
          >
            {navigationButton}
          </SimpleTooltip>
        ) : (
          navigationButton
        )}

        {/* Children - Chunking principle for better organization */}
        {hasChildren && isExpanded && !isCollapsed && (
          <div className="mt-0.5 space-y-0.5">
            {item.children!.map((childItem, index) => {
              const isLastChild = index === item.children!.length - 1;
              const newParentConnectors = [...parentConnectors];
              if (level >= 0) {
                newParentConnectors[level] = !isLastChild;
              }
              return renderNavigationItem(childItem, level + 1, isLastChild, newParentConnectors);
            })}
          </div>
        )}
      </div>
    );
  };

  return (
    <>
      <nav className={cn('space-y-1 overflow-x-hidden', className)} role="navigation" aria-label="Main navigation">
        {isCollapsed ? (
          // When collapsed, only show top-level items (Dashboard and Aquatiq)
          navigationItems.map((item) => 
            renderNavigationItem(item, 0, false, [])
          )
        ) : (
          // When expanded, show normal hierarchy
          navigationItems.map((item, index) => {
            const isLast = index === navigationItems.length - 1;
            return renderNavigationItem(item, 0, isLast, []);
          })
        )}
      </nav>
      
      {/* Floating Menu - conditionally render */}
      {/* eslint-disable-next-line react-hooks/refs -- Safe DOM measurement for floating menu positioning */}
      {isCollapsed && expandedFloatingMenu ? renderFloatingMenu() : null}
      
      {/* Click outside handler for floating menu */}
      {isCollapsed && expandedFloatingMenu && (
        <div 
          className="fixed inset-0 z-40" 
          onClick={() => onFloatingMenuChange?.(null)}
        />
      )}
    </>
  );
}
