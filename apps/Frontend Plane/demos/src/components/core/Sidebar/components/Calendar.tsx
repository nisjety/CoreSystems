'use client';

import React, { useState } from 'react';
import { CalendarEvent } from '../types';
import { cn } from '../utils';
import { 
  Calendar as CalendarIcon, 
  Clock,
  MapPin,
  Users,
  Plus
} from 'lucide-react';

interface CalendarProps {
  events: CalendarEvent[];
  selectedDate?: Date;
  onDateSelect: (date: Date) => void;
  onEventClick: (event: CalendarEvent) => void;
  onTitleClick?: () => void;
}

export function Calendar({ events, onEventClick, onTitleClick }: CalendarProps) {
  const [view, setView] = useState<'month' | 'week' | 'day'>('month');
  
  const today = new Date();
  const todayEvents = events.filter(event => 
    event.start.toDateString() === today.toDateString()
  );
  
  const upcomingEvents = events
    .filter(event => event.start > today)
    .sort((a, b) => a.start.getTime() - b.start.getTime())
    .slice(0, 5);

  const getEventStatusColor = (status: CalendarEvent['status']) => {
    switch (status) {
      case 'confirmed':
        return 'bg-green-100 text-green-800 border-green-200';
      case 'tentative':
        return 'bg-yellow-100 text-yellow-800 border-yellow-200';
      case 'cancelled':
        return 'bg-red-100 text-red-800 border-red-200';
      default:
        return 'bg-gray-100 text-gray-800 border-gray-200';
    }
  };

  const formatEventTime = (start: Date, end: Date) => {
    const startTime = start.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' });
    const endTime = end.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' });
    return `${startTime} - ${endTime}`;
  };

  return (
    <div className="flex flex-col h-full">
      {/* Header */}
      <div className="p-4 border-b border-white/10">
        <div className="flex items-center justify-between mb-4">
          <div className="flex items-center gap-2">
            <CalendarIcon className="w-5 h-5 text-white/60" />
            <h3 
              className={cn(
                "text-lg font-semibold",
                onTitleClick ? "text-white cursor-pointer hover:text-white/80 transition-colors" : "text-white"
              )}
              onClick={onTitleClick}
              title={onTitleClick ? "Go to Calendar page" : undefined}
            >
              Calendar
            </h3>
          </div>
          
          <button className="w-8 h-8 rounded-lg bg-white/20 hover:bg-white/30 flex items-center justify-center text-white transition-colors">
            <Plus className="w-4 h-4" />
          </button>
        </div>
        
        {/* View Toggle */}
        <div className="flex space-x-1 bg-white/10 rounded-lg p-1">
          {[
            { key: 'month', label: 'Month' },
            { key: 'week', label: 'Week' },
            { key: 'day', label: 'Day' }
          ].map((viewOption) => (
            <button
              key={viewOption.key}
              onClick={() => setView(viewOption.key as 'month' | 'week' | 'day')}
              className={cn(
                'flex-1 py-2 px-3 text-sm font-medium rounded-md transition-all duration-200',
                view === viewOption.key
                  ? 'bg-white/20 text-white shadow-sm'
                  : 'text-white/60 hover:text-white'
              )}
            >
              {viewOption.label}
            </button>
          ))}
        </div>
      </div>

      {/* Today's Events */}
      <div className="p-4 border-b border-white/10">
        <h4 className="text-sm font-semibold text-white mb-3 flex items-center gap-2">
          <Clock className="w-4 h-4" />
          Today&apos;s Events ({todayEvents.length})
        </h4>
        
        {todayEvents.length === 0 ? (
          <p className="text-sm text-white/60">No events today</p>
        ) : (
          <div className="space-y-2">
            {todayEvents.map((event) => (
              <div
                key={event.id}
                onClick={() => onEventClick(event)}
                className="flex items-start gap-3 p-3 rounded-lg border border-white/10 hover:border-white/20 cursor-pointer transition-all duration-200"
              >
                <div className="flex-shrink-0 w-2 h-2 bg-white/60 rounded-full mt-2"></div>
                <div className="flex-1 min-w-0">
                  <h5 className="text-sm font-medium text-white truncate">
                    {event.title}
                  </h5>
                  <p className="text-xs text-white/60">
                    {formatEventTime(event.start, event.end)}
                  </p>
                  {event.location && (
                    <p className="text-xs text-white/60 flex items-center gap-1 mt-1">
                      <MapPin className="w-3 h-3" />
                      {event.location}
                    </p>
                  )}
                </div>
              </div>
            ))}
          </div>
        )}
      </div>

      {/* Upcoming Events */}
      <div className="flex-1 overflow-y-auto p-4">
        <h4 className="text-sm font-semibold text-white mb-3">
          Upcoming Events
        </h4>
        
        {upcomingEvents.length === 0 ? (
          <div className="flex flex-col items-center justify-center h-full text-white/60">
            <CalendarIcon className="w-12 h-12 mb-4 text-white/30" />
            <p className="text-sm">No upcoming events</p>
          </div>
        ) : (
          <div className="space-y-3">
            {upcomingEvents.map((event) => (
              <div
                key={event.id}
                onClick={() => onEventClick(event)}
                className="p-3 rounded-xl border border-white/10 hover:border-white/20 cursor-pointer transition-all duration-200 hover:shadow-sm"
              >
                <div className="flex items-start justify-between mb-2">
                  <h5 className="text-sm font-medium text-white flex-1 truncate">
                    {event.title}
                  </h5>
                  <span className={cn(
                    'text-xs px-2 py-1 rounded-full border',
                    getEventStatusColor(event.status)
                  )}>
                    {event.status}
                  </span>
                </div>
                
                <div className="space-y-1">
                  <p className="text-xs text-white/60 flex items-center gap-1">
                    <Clock className="w-3 h-3" />
                    {event.start.toLocaleDateString()} • {formatEventTime(event.start, event.end)}
                  </p>
                  
                  {event.location && (
                    <p className="text-xs text-white/60 flex items-center gap-1">
                      <MapPin className="w-3 h-3" />
                      {event.location}
                    </p>
                  )}
                  
                  {event.attendees.length > 0 && (
                    <p className="text-xs text-white/60 flex items-center gap-1">
                      <Users className="w-3 h-3" />
                      {event.attendees.length} attendees
                    </p>
                  )}
                </div>
              </div>
            ))}
          </div>
        )}
      </div>
    </div>
  );
}