export function useGreeting(): string {
  const hour = new Date().getHours();
  if (hour < 12) return 'God morgen';
  if (hour < 18) return 'God ettermiddag';
  return 'God kveld';
}
