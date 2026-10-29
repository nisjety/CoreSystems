import { Composition } from "remotion";
import { SenseScene } from "../src/components/home/senses/SenseScene";
import { getSensePlayback } from "../src/components/home/senses/sense-player-playback";
import { VerevonProductShowcase } from "./VerevonProductShowcase";
import {
	type FeatureFilmKind,
	VerevonFeatureFilm,
} from "./VerevonFeatureFilm";
import {
	type ProductMomentKind,
	VerevonProductMoment,
} from "./VerevonProductMoment";
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

const productMoments: Array<{
	id: string;
	kind: ProductMomentKind;
}> = [
	{ id: "Verevon-Plan-Moment", kind: "plan" },
	{ id: "Verevon-Investigate-Moment", kind: "investigate" },
	{ id: "Verevon-FollowUp-Moment", kind: "followUp" },
];

export function RemotionRoot() {
	return (
		<>
			<Composition component={SenseScene} id="Senses-Delegering" defaultProps={{ kind: "delegate" }} durationInFrames={getSensePlayback("delegate").durationInFrames} fps={30} width={720} height={640} />
			<Composition component={SenseScene} id="Senses-Laering" defaultProps={{ kind: "learn" }} durationInFrames={getSensePlayback("learn").durationInFrames} fps={30} width={720} height={640} />
			<Composition component={SenseScene} id="Senses-Oversikt" defaultProps={{ kind: "oversee" }} durationInFrames={getSensePlayback("oversee").durationInFrames} fps={30} width={720} height={640} />
			<Composition component={VerevonProductShowcase} id="Verevon-Product" durationInFrames={240} fps={30} width={1280} height={800} />
			{productMoments.map(({ id, kind }) => (
				<Composition
					component={VerevonProductMoment}
					defaultProps={{ kind }}
					durationInFrames={216}
					fps={30}
					height={800}
					id={id}
					key={id}
					width={1280}
				/>
			))}
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
