import type { Metadata } from "next";
import { VerevonShortShowcase } from "@/components/showcase/VerevonShortShowcase";

export const metadata: Metadata = {
	title: "Showcase Short",
	description:
		"A vertical Verevon product showcase built for recording as a short-form video.",
};

export default function ShowcaseShortPage() {
	return <VerevonShortShowcase />;
}
