import type { MetadataRoute } from "next";
import { siteUrl } from "@/shared/seo/site";

const publicRoutes = [
	"/",
	"/produkt/arbeidsflyten",
	"/produkt/svartid",
	"/plattform/felles-kontekst",
	"/plattform/kontrollert-arbeid",
	"/trust",
];

export default function sitemap(): MetadataRoute.Sitemap {
	return publicRoutes.map((route) => ({
		changeFrequency: "monthly",
		lastModified: new Date("2026-09-08"),
		priority: route === "/" ? 1 : 0.8,
		url: new URL(route, siteUrl).toString(),
	}));
}
