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
	async headers() {
		return [
			{
				source: "/product-demo/:path*",
				headers: [
					{ key: "Content-Security-Policy", value: "default-src 'none'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data: blob:; font-src 'self' data:; connect-src 'none'; media-src 'self' blob:; worker-src 'none'; frame-src 'none'; object-src 'none'; manifest-src 'none'; base-uri 'none'; form-action 'none'; frame-ancestors 'self'" },
					{ key: "X-Robots-Tag", value: "noindex, nofollow" },
					{ key: "X-Content-Type-Options", value: "nosniff" },
					{ key: "Referrer-Policy", value: "no-referrer" },
					{ key: "Permissions-Policy", value: "geolocation=(), microphone=(), camera=(), payment=(), usb=(), serial=(), bluetooth=(), hid=(), midi=(), magnetometer=(), gyroscope=(), accelerometer=(), display-capture=()" },
					{ key: "Access-Control-Allow-Origin", value: "*" },
					{ key: "Cache-Control", value: "public, max-age=0, must-revalidate" },
				],
			},
			{
				source: "/product-demo/assets/:path*",
				headers: [{ key: "Cache-Control", value: "public, max-age=31536000, immutable" }],
			},
		];
	},
	async rewrites() {
		return [{ source: "/product-demo", destination: "/product-demo/index.html" }];
	},
};

export default nextConfig;
