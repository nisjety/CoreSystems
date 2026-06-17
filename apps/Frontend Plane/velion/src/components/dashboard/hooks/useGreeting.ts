export function getGreeting(now = new Date()): string {
  const hour = now.getHours();
  if (hour < 12) return 'God morgen';
  if (hour < 18) return 'God ettermiddag';
  return 'God kveld';
}

export function useGreeting(): string {
  return getGreeting();
}
