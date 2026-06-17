export function SkeletonLoader({ lines = 3 }: { lines?: number }) {
  return (
    <div className="space-y-2">
      {[...Array(lines)].map((_, i) => (
        <div key={i} className="h-4 bg-gray-200 rounded animate-pulse w-full" style={{ width: `${85 + Math.random() * 15}%` }} />
      ))}
    </div>
  );
}
