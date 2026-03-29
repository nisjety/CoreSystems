// Simplified verification hook for now
// This can be expanded later with real implementation

import { useState } from 'react';

export function useVerification() {
  const [isLoading] = useState(false);

  return {
    isLoading,
  };
}
