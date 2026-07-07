import type { Metadata } from "next";
import { VelionShortShowcase } from "@/components/showcase/VelionShortShowcase";

export const metadata: Metadata = {
	title: "Showcase Short",
	description:
		"A vertical Velion product showcase built for recording as a short-form video.",
};

export default function ShowcaseShortPage() {
	return <VelionShortShowcase />;
}
