import { Composition } from "remotion";
import { SenseScene } from "../src/components/home/senses/SenseScene";
import { VerevonProductShowcase } from "./VerevonProductShowcase";
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
			<Composition component={SenseScene} id="Senses-Delegering" defaultProps={{ kind: "delegate" }} durationInFrames={540} fps={30} width={720} height={640} />
			<Composition component={SenseScene} id="Senses-Laering" defaultProps={{ kind: "learn" }} durationInFrames={540} fps={30} width={720} height={640} />
			<Composition component={SenseScene} id="Senses-Oversikt" defaultProps={{ kind: "oversee" }} durationInFrames={540} fps={30} width={720} height={640} />
			<Composition component={VerevonProductShowcase} id="Verevon-Product" durationInFrames={240} fps={30} width={1280} height={800} />
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
