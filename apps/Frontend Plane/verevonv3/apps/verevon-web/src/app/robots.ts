import type { MetadataRoute } from "next";
import { siteUrl } from "@/shared/seo/site";

export default function robots(): MetadataRoute.Robots {
	return {
		rules: [
			{ userAgent: "GPTBot", disallow: "/" },
			{ userAgent: "Google-Extended", disallow: "/" },
			{ userAgent: "OAI-SearchBot", allow: "/" },
			{ userAgent: "*", allow: "/" },
		],
		sitemap: new URL("/sitemap.xml", siteUrl).toString(),
	};
}
