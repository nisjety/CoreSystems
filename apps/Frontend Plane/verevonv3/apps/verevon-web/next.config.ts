import type { NextConfig } from "next";
import path from "node:path";

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
	// Vercel resolves traced files from the Git repository root.
	outputFileTracingRoot: path.resolve(process.cwd(), "../../../../.."),
	reactCompiler: true,
	experimental: {
		viewTransition: true,
	},
};

export default nextConfig;
