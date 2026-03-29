import React from 'react';

export interface AttachmentOption {
  id: string;
  label: string;
  icon: React.ComponentType<{ className?: string }>;
  description: string;
}

export interface ToolOption {
  id: string;
  label: string;
  icon: React.ComponentType<{ className?: string }>;
  description: string;
}

export interface HistoryOption {
  id: string;
  label: string;
  timestamp: string;
  preview: string;
}
