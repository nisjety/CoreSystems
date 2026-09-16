import type { NextConfig } from "next";

const nextConfig: NextConfig = {
	images: {
		qualities: [65, 75, 90],
		remotePatterns: [
			{
				protocol: "https",
				hostname: "images.pexels.com",
				pathname: "/photos/**",
			},
			{
				protocol: "https",
				hostname: "images.unsplash.com",
				pathname: "/**",
			},
		],
	},
	// This app has its own dependencies; keep deployment paths local to it.
	outputFileTracingRoot: process.cwd(),
	reactCompiler: true,
	experimental: {
		viewTransition: true,
	},
};

export default nextConfig;
