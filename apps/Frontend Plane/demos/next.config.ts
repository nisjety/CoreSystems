
/** @type {import('next').NextConfig} */
const nextConfig = {
  output: "standalone",
  typescript: {
    ignoreBuildErrors: true,
  },
  
  experimental: {
    optimizeCss: false,
    webpackBuildWorker: false,
  },

  compiler: {
    removeConsole: false, // Keep console logs for debugging
  },

  images: {
    formats: ['image/webp', 'image/avif'],
    minimumCacheTTL: 3600,
    dangerouslyAllowSVG: true,
    contentDispositionType: 'attachment',
    contentSecurityPolicy: "default-src 'self'; script-src 'none'; sandbox;",
    
    remotePatterns: [
      {
        protocol: 'https',
        hostname: 'lh3.googleusercontent.com',
        port: '',
        pathname: '/**',
      },
      {
        protocol: 'https',
        hostname: 'graph.microsoft.com',
        port: '',
        pathname: '/**',
      },
      {
        protocol: 'https',
        hostname: 'avatars.githubusercontent.com',
        port: '',
        pathname: '/**',
      },
      {
        protocol: 'https',
        hostname: 'cdn.discordapp.com',
        port: '',
        pathname: '/**',
      },
      {
        protocol: 'https',
        hostname: 'cdn.prod.website-files.com',
        port: '',
        pathname: '/**',
      },
    ],
  },

  env: {
    BACKEND_URL: process.env.BACKEND_URL || 'http://localhost:3011',
    RISK_CONTROL_API_URL: process.env.RISK_CONTROL_API_URL || 'http://127.0.0.1:8095',
    CONTRACT_MANAGEMENT_URL: process.env.CONTRACT_MANAGEMENT_URL || 'http://aquatiq-contract-manager-dev:8001',
    XERO_SERVICE_URL: process.env.XERO_SERVICE_URL || 'http://localhost:8005',
    CONTIFICO_SERVICE_URL: process.env.CONTIFICO_SERVICE_URL || 'http://localhost:8006',
  },

  async rewrites() {
    return [
      {
        source: '/api/contracts/:path*',
        destination: `${process.env.CONTRACT_MANAGEMENT_URL || 'http://aquatiq-contract-manager-dev:8001'}/:path*`,
      },
      {
        source: '/api/xero/:path*',
        destination: `${process.env.XERO_SERVICE_URL || 'http://localhost:8005'}/api/xero/:path*`,
      },
      {
        source: '/api/contifico/:path*',
        destination: `${process.env.CONTIFICO_SERVICE_URL || 'http://localhost:8006'}/:path*`,
      },
    ];
  },

  async headers() {
    return [
      {
        source: '/contract-admin',
        headers: [
          {
            key: 'Content-Security-Policy',
            value: "frame-src 'self' http://localhost:8001 https://tools.aquatiq.com;",
          },
        ],
      },
    ];
  },
};

export default nextConfig;
