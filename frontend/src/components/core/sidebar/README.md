# Modern Aquatiq Sidebar

A sophisticated, modern sidebar component built with React, TypeScript, and Tailwind CSS. Features Microsoft Teams, Outlook, and Calendar integration for a unified workspace experience.

## Features

### 🎨 Modern Design
- Clean white design with subtle shadows
- Smooth animations and transitions
- Responsive and accessible UI
- Consistent design system

### 🔗 Microsoft Integration
- **Teams Messages**: Real-time messaging with Microsoft Teams
- **Outlook Email**: Email notifications and management
- **Calendar**: Calendar events and scheduling
- **Notifications**: Unified notification system

### ⚡ Smart Features
- Real-time updates
- Search functionality
- Priority-based notifications
- Status indicators
- Badge notifications

### 🛠 Technical Features
- Full TypeScript support
- tRPC for type-safe API calls
- React Query for data fetching
- Tailwind CSS for styling
- Component-based architecture

## Components

### Core Components
- `index.tsx` - Main sidebar component (supports both minimized and expanded states)
- `UserProfile` - User avatar and status
- `Navigation` - Navigation menu with badges
- `MessageTab` - Teams and email messages
- `NotificationPanel` - Unified notifications
- `Calendar` - Calendar events and scheduling
- `Settings` - Settings menu
- `SettingsPanel` - Settings configuration panel
- `ChatHistoryTab` - Chat history interface

### Minimized State Components
- `MinimizedLogo` - Logo with expand toggle for minimized state
- `MinimizedNavigation` - Navigation icons for minimized state
- `MinimizedFooter` - Help and profile buttons for minimized state

### UI Components
- `SimpleTooltip` - Tooltip component for minimized state

## Usage

```tsx
import { Sidebar } from './components/Sidebar';

function App() {
  return (
    <Sidebar 
      user={{
        id: '1',
        name: 'Ima Fernandes Da Costa',
        email: 'ima@aquatiq.com',
        position: 'IT Consultant',
        status: 'online'
      }}
      onNavigate={(item) => {
        // Handle navigation
        console.log('Navigate to:', item);
      }}
    />
  );
}
```

## Configuration

### Microsoft Graph Setup
To enable Microsoft integration, configure the following environment variables:

```env
NEXT_PUBLIC_MICROSOFT_CLIENT_ID=your_client_id
NEXT_PUBLIC_MICROSOFT_AUTHORITY=https://login.microsoftonline.com/your_tenant_id
NEXT_PUBLIC_MICROSOFT_REDIRECT_URI=http://localhost:3001/auth/callback
```

### tRPC Backend
The sidebar uses tRPC for type-safe API communication. The backend includes:

- User management
- Message fetching
- Notification handling
- Calendar events
- Real-time updates

## Styling

The sidebar uses Tailwind CSS with a custom design system:

- **Primary Colors**: Blue gradient (#3B82F6 to #8B5CF6)
- **Typography**: Clean, modern font stack
- **Shadows**: Subtle shadow system
- **Animations**: Smooth 200ms transitions

## Data Flow

```
Sidebar → tRPC Client → Backend API → Microsoft Graph → Office 365
```

1. User interactions trigger tRPC calls
2. Backend fetches data from Microsoft Graph
3. Real-time updates via WebSocket/polling
4. UI updates with React Query cache

## Development

### Prerequisites
- Node.js 18+
- Next.js 15+
- TypeScript 5+

### Installation
```bash
npm install
npm run dev
```

### Building
```bash
npm run build
```

## Architecture

### File Structure
```
src/components/Sidebar/
├── index.tsx                      # Main sidebar component (both states)
├── types.ts                       # TypeScript type definitions
├── components/                    # Sub-components
│   ├── UserProfile.tsx
│   ├── Navigation.tsx
│   ├── MessageTab.tsx
│   ├── NotificationPanel.tsx
│   ├── Calendar.tsx
│   ├── ChatHistoryTab.tsx
│   ├── Settings.tsx
│   ├── SettingsPanel.tsx
│   ├── MinimizedLogo.tsx         # Minimized state
│   ├── MinimizedNavigation.tsx   # Minimized state
│   └── MinimizedFooter.tsx       # Minimized state
├── hooks/                         # Custom hooks
│   ├── useItems.ts
│   └── useRealData.ts            # Real API data hooks
├── config/                        # Configuration
│   └── nav-items.ts              # Navigation items
├── utils/                         # Utility functions
│   ├── index.ts
│   └── cx.ts
├── lib/                           # Libraries
│   └── utils.ts                  # Utility functions
└── ui/                            # UI components
    └── simple-tooltip.tsx
```

### State Management
- **React Query**: Server state and caching via useRealData hooks
- **React State**: Local component state
- **SidebarContext**: Shared state for sidebar minimize/expand
- **GlobalLanguageContext**: i18n language state

### Performance
- Lazy loading of components
- Optimized re-renders
- Efficient data fetching
- Image optimization

## Contributing

1. Follow TypeScript best practices
2. Use Tailwind CSS for styling
3. Add proper TypeScript interfaces
4. Include unit tests
5. Update documentation

## License

Private - Aquatiq Internal Use
