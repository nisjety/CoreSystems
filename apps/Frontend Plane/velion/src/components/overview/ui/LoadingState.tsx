export function LoadingState() {
  return (
    <div className="min-h-screen bg-[#F7F7FA] p-6">
      <div className="max-w-7xl mx-auto">
        <div className="mb-8">
          <div className="h-8 bg-gray-200 rounded animate-pulse w-48 mb-2" />
          <div className="h-4 bg-gray-100 rounded animate-pulse w-96" />
        </div>

        <div className="grid gap-4 md:grid-cols-2 lg:grid-cols-3">
          {[...Array(6)].map((_, i) => (
            <div key={i} className="rounded-[22px] border border-[#E6E8EF] bg-white p-6 animate-pulse">
              <div className="h-4 bg-gray-200 rounded w-24 mb-4" />
              <div className="h-8 bg-gray-100 rounded w-32 mb-4" />
              <div className="flex gap-2">
                <div className="h-6 bg-gray-100 rounded w-16" />
              </div>
            </div>
          ))}
        </div>
      </div>
    </div>
  );
}
