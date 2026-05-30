export function formatDateKey(date: Date) {
  return date.toISOString().slice(0, 10);
}
