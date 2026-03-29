interface UserStatsProps {
  stats: {
    totalUsers: number;
    activeUsers: number;
    totalOrganizations: number;
    totalApiKeys: number;
    activeSessions: number;
    recentSignUps: number;
    recentLogins: number;
  };
}

export function UserStats({ stats }: UserStatsProps) {
  const statCards = [
    {
      title: 'Total Users',
      value: stats.totalUsers,
      icon: '👥',
      color: 'blue'
    },
    {
      title: 'Active Users',
      value: stats.activeUsers,
      icon: '✅',
      color: 'green'
    },
    {
      title: 'Organizations',
      value: stats.totalOrganizations,
      icon: '🏢',
      color: 'purple'
    },
    {
      title: 'API Keys',
      value: stats.totalApiKeys,
      icon: '🔑',
      color: 'orange'
    },
    {
      title: 'Active Sessions',
      value: stats.activeSessions,
      icon: '🔗',
      color: 'indigo'
    },
    {
      title: 'Recent Sign Ups',
      value: stats.recentSignUps,
      icon: '📈',
      color: 'pink'
    },
    {
      title: 'Recent Logins',
      value: stats.recentLogins,
      icon: '🔄',
      color: 'teal'
    }
  ];

  const getColorClasses = (color: string) => {
    const colorMap: Record<string, string> = {
      blue: 'bg-blue-50 border-blue-200 text-blue-800',
      green: 'bg-green-50 border-green-200 text-green-800',
      purple: 'bg-purple-50 border-purple-200 text-purple-800',
      orange: 'bg-orange-50 border-orange-200 text-orange-800',
      indigo: 'bg-indigo-50 border-indigo-200 text-indigo-800',
      pink: 'bg-pink-50 border-pink-200 text-pink-800',
      teal: 'bg-teal-50 border-teal-200 text-teal-800'
    };
    return colorMap[color] || colorMap.blue;
  };

  return (
    <div className="grid grid-cols-1 md:grid-cols-3 lg:grid-cols-4 gap-4">
      {statCards.map((stat) => (
        <div
          key={stat.title}
          className={`rounded-lg border p-4 ${getColorClasses(stat.color)}`}
        >
          <div className="flex items-center justify-between">
            <div>
              <p className="text-sm font-medium opacity-75">{stat.title}</p>
              <p className="text-2xl font-bold">{stat.value.toLocaleString()}</p>
            </div>
            <div className="text-2xl">{stat.icon}</div>
          </div>
        </div>
      ))}
    </div>
  );
}