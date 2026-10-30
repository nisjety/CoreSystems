export type ProductMomentName = "plan" | "investigate" | "followUp";

type ProductMomentMedia = {
	alt: string;
	kind: "video";
	poster: string;
	src: string;
};

// These are intentionally illustrative product moments. The accompanying
// section copy supplies the textual equivalent; the videos make the actions
// easier to understand without implying a live customer run.
export const PRODUCT_MOMENT_MEDIA: Record<
	ProductMomentName,
	ProductMomentMedia
> = {
	plan: {
		kind: "video",
		src: "/verevon-product-moments/plan.mp4",
		poster: "/verevon-product-moments/plan-poster.jpg",
		alt: "Illustrert Verevon-flyt der et spørsmål blir til et planutkast med ansvar, avhengigheter og frister.",
	},
	investigate: {
		kind: "video",
		src: "/verevon-product-moments/investigate.mp4",
		poster: "/verevon-product-moments/investigate-poster.jpg",
		alt: "Illustrert Verevon-flyt som kobler en blokkering til kilder og synliggjør en endring som kan testes.",
	},
	followUp: {
		kind: "video",
		src: "/verevon-product-moments/follow-up.mp4",
		poster: "/verevon-product-moments/follow-up-poster.jpg",
		alt: "Illustrert Verevon-flyt der et underbygd utkast blir gjennomgått og klargjort for oppfølging.",
	},
};
