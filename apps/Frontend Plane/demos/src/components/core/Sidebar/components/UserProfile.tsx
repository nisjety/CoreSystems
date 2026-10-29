'use client';

import React from 'react';
import { cn, getAvatarGradient } from '../utils';
import { useCompatibleLanguage } from '@/components/shared/contexts/GlobalLanguageContext';

// User type definition
interface User {
  id: string;
  name: string;
  email: string;
  position?: string;
  avatar?: string;
  status: 'online' | 'away' | 'busy' | 'offline';
}

interface UserProfileProps {
  user: User;
  showDetails?: boolean;
  onClick?: () => void;
  className?: string;
  isMinimized?: boolean;
  showStatusIndicator?: boolean;
  size?: 'small' | 'medium' | 'large';
}

// i18n translations
const translations = {
  en: {
    online: 'Online',
    away: 'Away', 
    busy: 'Busy',
    offline: 'Offline'
  },
  no: {
    online: 'Pålogget',
    away: 'Borte',
    busy: 'Opptatt', 
    offline: 'Frakoblet'
  }
};

// Status colors following Von Restorff Effect - distinctive visual indicators
const getStatusColor = (status: User['status']) => {
  switch (status) {
    case 'online':
      return 'bg-green-500';
    case 'away':
      return 'bg-yellow-500';
    case 'busy':
      return 'bg-red-500';
    case 'offline':
    default:
      return 'bg-gray-400';
  }
};

// Avatar size configurations following Fitts's Law
const getSizeConfig = (size: 'small' | 'medium' | 'large', isMinimized: boolean) => {
  if (isMinimized) {
    return {
      avatar: 'w-8 h-8',
      status: 'w-2.5 h-2.5 -bottom-0.5 -right-0.5',
      text: 'text-xs',
      nameText: 'text-xs',
      subtitleText: 'text-xs'
    };
  }

  switch (size) {
    case 'small':
      return {
        avatar: 'w-5 h-5',
        status: 'w-1.5 h-1.5 -bottom-0 -right-0',
        text: 'text-xs',
        nameText: 'text-xs',
        subtitleText: 'text-xs'
      };
    case 'large':
      return {
        avatar: 'w-10 h-10',
        status: 'w-3 h-3 -bottom-0.5 -right-0.5',
        text: 'text-sm',
        nameText: 'text-sm',
        subtitleText: 'text-xs'
      };
    case 'medium':
    default:
      return {
        avatar: 'w-8 h-8',
        status: 'w-2.5 h-2.5 -bottom-0.5 -right-0.5',
        text: 'text-xs',
        nameText: 'text-xs',
        subtitleText: 'text-xs'
      };
  }
};

export default function UserProfile({
  user,
  onClick,
  showDetails = true,
  isMinimized = false,
  size = 'medium',
  className
}: UserProfileProps) {
  const { sidebarLocale } = useCompatibleLanguage(); // Get language from global context
  
  // Translation helper
  const t = (key: string): string => {
    const langTranslations = translations[sidebarLocale as keyof typeof translations] || translations['en'];
    return langTranslations[key as keyof typeof langTranslations] || key;
  };
  
  // Size configurations
  const sizeConfig = getSizeConfig(size, isMinimized);
  
  // Status indicator visibility
  const showStatusIndicator = user.status && user.status !== 'online';
  
  return (
    <div 
      className={cn(
        // Base styles following Law of Common Region
        'flex items-center transition-all duration-200 group',
        
        // Responsive spacing and centering for minimized state
        isMinimized ? 'justify-center p-3' : 'gap-3 p-4',
        
        // Interactive states - following Aesthetic-Usability Effect
        onClick && 'cursor-pointer hover:bg-white rounded-xl',
        
        // Touch-friendly sizing - following Fitts's Law
        onClick && 'min-h-[44px]',
        
        className
      )}
      onClick={onClick}
      role={onClick ? 'button' : undefined}
      tabIndex={onClick ? 0 : undefined}
      onKeyDown={onClick ? (e) => {
        if (e.key === 'Enter' || e.key === ' ') {
          e.preventDefault();
          onClick();
        }
      } : undefined}
      aria-label={onClick ? `View ${user.name}'s profile` : undefined}
    >
      {/* Avatar container with status indicator - Always centered when minimized */}
      <div className={cn(
        'relative flex-shrink-0',
        isMinimized && 'mx-auto'
      )}>
        <div 
          className={cn(
            sizeConfig.avatar,
            `bg-gradient-to-br ${getAvatarGradient(user.name)}`,
            'rounded-full flex items-center justify-center shadow-sm',
            'transition-transform duration-200 group-hover:scale-105'
          )}
        >
          {user.avatar ? (
            <img 
              src={user.avatar} 
              alt={user.name}
              className={cn(sizeConfig.avatar, 'rounded-full object-cover')}
              loading="lazy"
            />
          ) : (
            <span className={cn('text-white font-medium', sizeConfig.text)}>
              {user.name.charAt(0).toUpperCase()}
            </span>
          )}
        </div>
        
        {/* Status indicator - Following Von Restorff Effect for clear status communication */}
        {showStatusIndicator && (
          <div 
            className={cn(
              'absolute rounded-full border-2 border-white transition-colors duration-200',
              sizeConfig.status,
              getStatusColor(user.status)
            )}
            title={`${user.name} - ${t(user.status)}`}
            aria-label={`Status: ${t(user.status)}`}
          />
        )}
      </div>

      {/* User details - Following Chunking principle for information organization */}
      {!isMinimized && showDetails && (
        <div className="flex-1 min-w-0">
          <h3 className={cn(
            'font-semibold text-white truncate transition-colors duration-200',
            sizeConfig.nameText,
            onClick && 'group-hover:text-[#151F6C]'
          )}>
            {user.name}
          </h3>
          
          <p className={cn(
            'text-white/70 truncate group-hover:text-[#151F6C]/70 transition-colors duration-200',
            sizeConfig.subtitleText
          )}>
            {user.position || user.email}
          </p>
          
          {/* Status text for accessibility - Following inclusive design principles */}
          <span className="sr-only">
            Status: {t(user.status)}
          </span>
        </div>
      )}

      {/* Tooltip for minimized state */}
      {isMinimized && (
        <div className="absolute left-full ml-2 px-3 py-2 bg-gray-900 text-white text-sm rounded-lg opacity-0 group-hover:opacity-100 transition-opacity duration-200 pointer-events-none whitespace-nowrap z-50">
          <div className="font-medium">{user.name}</div>
          <div className="text-xs text-gray-300">{user.position || user.email}</div>
          <div className="text-xs text-gray-300">Status: {t(user.status)}</div>
          {/* Tooltip arrow */}
          <div className="absolute left-0 top-1/2 transform -translate-x-1 -translate-y-1/2 w-0 h-0 border-t-4 border-b-4 border-r-4 border-t-transparent border-b-transparent border-r-gray-900"></div>
        </div>
      )}
    </div>
  );
}

// Additional utility function for batch user display
export function UserProfileList({ 
  users, 
  onUserClick,
  maxDisplay = 5,
  showMoreLabel = 'Show more',
  isMinimized = false
}: {
  users: User[];
  onUserClick?: (user: User) => void;
  maxDisplay?: number;
  showMoreLabel?: string;
  isMinimized?: boolean;
}) {
  const { sidebarLocale } = useCompatibleLanguage(); // Get language from global context
  const displayUsers = users.slice(0, maxDisplay);
  const remainingCount = users.length - maxDisplay;

  return (
    <div className={cn(
      'space-y-1',
      // Following Miller's Law - limit displayed items to manageable chunks
      isMinimized && 'space-y-2'
    )}>
      {displayUsers.map((user) => (
        <UserProfile 
          key={user.id}
          user={user}
          onClick={onUserClick ? () => onUserClick(user) : undefined}
          showDetails={!isMinimized}
          isMinimized={isMinimized}
          size="small"
        />
      ))}
      
      {/* Show more indicator - Following Goal-Gradient Effect */}
      {remainingCount > 0 && (
        <div className={cn(
          'text-center py-2',
          isMinimized ? 'px-2' : 'px-4'
        )}>
          <button 
            className="text-sm text-white hover:text-[#151F6C] font-medium transition-colors duration-200 hover:bg-white px-2 py-1 rounded"
            onClick={() => {
              // Could expand to show more users or navigate to full user list
              console.log(`Show ${remainingCount} more users`);
            }}
          >
            {isMinimized ? `+${remainingCount}` : `${showMoreLabel} (${remainingCount})`}
          </button>
        </div>
      )}
    </div>
  );
}