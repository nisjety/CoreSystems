import {
	VEREVON_MARK_LEFT_V_PATH,
	VEREVON_MARK_RIGHT_WING_PATH,
} from "@/components/home/sections/VerevonMark";

type VerevonBrandMarkProps = {
	className?: string;
};

/** The cropped, canonical mark used beside the permanent site wordmark. */
export function VerevonBrandMark({ className = "" }: VerevonBrandMarkProps) {
	return (
		<svg
			aria-hidden="true"
			className={className}
			fill="currentColor"
			viewBox="20 26 82 68"
		>
			<path d={VEREVON_MARK_LEFT_V_PATH} />
			<path d={VEREVON_MARK_RIGHT_WING_PATH} />
		</svg>
	);
}
