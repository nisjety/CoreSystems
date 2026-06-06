import type { NextConfig } from "next";

const nextConfig: NextConfig = {
  // React Compiler runs as a Babel pass, which bypasses the fast SWC/Turbopack
  // transform and adds multi-second-per-route compile overhead in dev. Keep it
  // for production builds (where the optimization matters and compile time is
  // paid once), but disable it in dev for fast Turbopack compiles.
  reactCompiler: process.env.NODE_ENV === "production",
  typedRoutes: true,
  poweredByHeader: false,
  compress: true,
  images: {
    formats: ["image/avif", "image/webp"],
    minimumCacheTTL: 3600,
    remotePatterns: [
      {
        protocol: "https",
        hostname: "images.unsplash.com",
        pathname: "/**",
      },
      {
        protocol: "https",
        hostname: "graph.microsoft.com",
        pathname: "/**",
      },
      {
        protocol: "https",
        hostname: "lh3.googleusercontent.com",
        pathname: "/**",
      },
    ],
  },
};

export default nextConfig;
