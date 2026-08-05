import { Composition } from "remotion";
import {
	type FeatureFilmKind,
	VerevonFeatureFilm,
} from "./VerevonFeatureFilm";
import {
	DURATION_IN_FRAMES,
	FPS,
	HEIGHT,
	WIDTH,
} from "./timeline";

const compositions: Array<{
	id: string;
	kind: FeatureFilmKind;
}> = [
	{ id: "Verevon-Build", kind: "build" },
	{ id: "Verevon-Connect", kind: "connect" },
	{ id: "Verevon-Ground", kind: "ground" },
	{ id: "Verevon-Approve", kind: "approve" },
];

export function RemotionRoot() {
	return (
		<>
			{compositions.map(({ id, kind }) => (
				<Composition
					component={VerevonFeatureFilm}
					defaultProps={{ kind }}
					durationInFrames={DURATION_IN_FRAMES}
					fps={FPS}
					height={HEIGHT}
					id={id}
					key={id}
					width={WIDTH}
				/>
			))}
		</>
	);
}
