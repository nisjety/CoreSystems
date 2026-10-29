import type { CSSProperties, ReactNode } from "react";
import { AbsoluteFill, Easing, interpolate, Sequence, staticFile, useCurrentFrame } from "remotion";
import { ArrowUpRight, Check, FileText, Mail, Search, ShieldCheck, Sparkles } from "lucide-react";
import { VerevonMark } from "../sections/VerevonMark";
import { getStageOpacity } from "./sense-player-playback";

export type SenseKind = "delegate" | "learn" | "oversee";
export const SENSE_FPS = 30;

// The illustrations share the site's materials; they do not depict a live run.
const ink = "#272623";
const muted = "#74706a";
const clay = "#a85538";
const line = "rgba(48,39,30,.11)";
const paper = "#fffefa";
const ease = Easing.bezier(0.16, 1, 0.3, 1);
const clamp = { extrapolateLeft: "clamp", extrapolateRight: "clamp", easing: ease } as const;
const panel: CSSProperties = { background: paper, border: "1px solid rgba(255,255,255,.85)", borderRadius: 20, boxShadow: "0 2px 5px #32231708, 0 24px 70px #32231724", overflow: "hidden" };
const row: CSSProperties = { display: "flex", alignItems: "center", gap: 14 };

function Brand({ small = false }: { small?: boolean }) {
	return <span style={{ display: "inline-grid", placeItems: "center", flexShrink: 0, width: small ? 30 : 44, height: small ? 30 : 44, background: ink, color: paper, borderRadius: small ? 8 : 12 }}><VerevonMark style={{ display: "block", width: "100%", height: "100%", transform: "scale(1.3)" }} /></span>;
}

function Chip({ children, done = false }: { children: ReactNode; done?: boolean }) {
	return <span style={{ ...row, display: "inline-flex", gap: 7, padding: "7px 12px", borderRadius: 30, background: done ? "#e8ede6" : "#f2e7df", color: done ? "#426049" : clay, fontSize: 17, whiteSpace: "nowrap", lineHeight: 1.2 }}>{done ? <Check size={16} /> : <span style={{ width: 5, height: 5, borderRadius: "50%", background: "currentColor" }} />}{children}</span>;
}

function Enter({ at, children, style, until = 540 }: { at: number; until?: number; children: ReactNode; style?: CSSProperties }) {
	const frame = useCurrentFrame();
	const enter = interpolate(frame, [at, at + 20], [0, 1], clamp);
	const exit = interpolate(frame, [until - 14, until], [1, 0], clamp);
	return <div style={{ ...style, opacity: enter * exit, transform: `translateY(${(1 - enter) * 22 - (1 - exit) * 10}px) scale(${0.96 + enter * 0.04})` }}>{children}</div>;
}

function Meter({ start, end, color = clay }: { start: number; end: number; color?: string }) {
	const frame = useCurrentFrame();
	return <div style={{ height: 3, background: "#2927230d", overflow: "hidden", borderRadius: 3 }}><div style={{ height: "100%", background: color, transformOrigin: "left", transform: `scaleX(${interpolate(frame, [start, end], [0, 1], clamp)})` }} /></div>;
}

function ResultRow({ at, label, sub }: { at: number; label: string; sub: string }) {
	const frame = useCurrentFrame();
	const complete = frame >= at + 38;
	return <Enter at={at} style={{ ...row, padding: "12px 0", borderTop: `1px solid ${line}` }}>
		<span style={{ display: "grid", placeItems: "center", width: 31, height: 31, borderRadius: 10, background: complete ? "#e8ede6" : "#f3ebe5", color: complete ? "#426049" : clay }}>{complete ? <Check size={19} /> : <span style={{ width: 12, height: 12, border: `1.5px solid ${clay}`, borderRightColor: "transparent", borderRadius: "50%", transform: `rotate(${frame * 7}deg)` }} />}</span>
		<div style={{ flex: 1 }}><div style={{ fontSize: 22 }}>{label}</div><div style={{ color: muted, fontSize: 17, marginTop: 3 }}>{sub}</div></div>
	</Enter>;
}

function Stage({ kind, children }: { kind: SenseKind; children: ReactNode }) {
	const frame = useCurrentFrame();
	return <AbsoluteFill style={{ fontFamily: "Manrope, Arial, sans-serif", fontSize: 23, color: ink, fontWeight: 400, lineHeight: 1.3, fontVariantNumeric: "tabular-nums", background: "transparent", overflow: "hidden" }}>
		<style>{`@font-face{font-family:Manrope;src:url('${staticFile("fonts/manrope/manrope-latin-variable.woff2")}') format('woff2');font-weight:200 800}@font-face{font-family:Geist;src:url('${staticFile("fonts/geist/geist-variable.woff2")}') format('woff2');font-weight:100 900}`}</style>
		<AbsoluteFill style={{ background: kind === "oversee" ? "linear-gradient(145deg,#edf2f1,#dce7e7)" : "linear-gradient(150deg,#ffffff26,transparent 70%,#c3916726)" }} />
		{kind === "delegate" ? <svg viewBox="0 0 720 640" style={{ position: "absolute", inset: 0, width: "100%", height: "100%", opacity: .27 }} fill="none">
			<path d="M-20 488 H74 Q98 488 98 464 V95 Q98 69 124 69 H606 Q632 69 632 95 V568 Q632 590 654 590 H740" stroke={clay} strokeWidth="1" />
			<path d="M-20 488 H74 Q98 488 98 464 V95 Q98 69 124 69 H606 Q632 69 632 95 V568 Q632 590 654 590 H740" stroke={clay} strokeWidth="2.5" strokeDasharray="45 1700" strokeDashoffset={-frame * 3.2} />
		</svg> : null}
		<AbsoluteFill style={{ opacity: getStageOpacity(kind, frame) }}>{children}</AbsoluteFill>
	</AbsoluteFill>;
}

function DelegateScene() {
	const frame = useCurrentFrame();
	const extracted = frame >= 180;
	const complete = frame >= 382;
	return <Stage kind="delegate">
		<Sequence name="Deleger oppgaven" layout="none">
			<Enter at={0} style={{ ...panel, ...row, position: "absolute", top: 65, left: 75, width: 566, padding: "18px 22px", background: "#fffefaee" }}>
				<span style={{ width: 34, height: 34, borderRadius: "50%", background: "#dfd5c8", display: "grid", placeItems: "center", fontSize: 17 }}>Du</span>
				<span style={{ flex: 1, fontSize: 24 }}>Følg opp ubetalte fakturaer.</span><ArrowUpRight size={24} />
			</Enter>
		</Sequence>
		<Sequence name="Søk i innboksen" layout="none">
			<Enter at={24} style={{ ...panel, position: "absolute", top: 168, left: 102, width: 508, padding: "22px 26px", opacity: .8, filter: extracted ? "blur(1.5px)" : undefined, transformOrigin: "center top" }}>
				<div style={{ ...row, justifyContent: "space-between", marginBottom: 19 }}><span style={row}><Brand small /><span style={{ fontSize: 20 }}>Verevon arbeider</span></span><Search size={20} color={clay} /></div>
				<div style={{ color: muted, fontSize: 19 }}>{extracted ? "Kobler e-post og regnskap …" : "Søker i innboksen …"}</div>
				<div style={{ marginTop: 16 }}><Meter start={32} end={168} /></div>
			</Enter>
		</Sequence>
		<Sequence name="Faktura og nøkkeldata" layout="none">
			<Enter at={85} style={{ ...panel, position: "absolute", top: 263, left: 50, width: 620, padding: "26px 28px" }}>
				<div style={{ ...row, justifyContent: "space-between", marginBottom: 22 }}><span style={{ ...row, fontSize: 18, color: muted }}><Mail size={23} /> Innboks / Økonomi</span><span style={{ color: muted, fontSize: 17 }}>14. sep.</span></div>
				<div style={{ fontSize: 30, letterSpacing: "-.035em" }}>Oppfølging av faktura</div>
				<div style={{ color: muted, fontSize: 21, marginTop: 6 }}>Nordlys Design · INV-2026-0847</div>
				<div style={{ display: "grid", gridTemplateColumns: "1fr 1fr", gap: 16, marginTop: 23 }}>
					<Enter at={125} style={{ padding: "14px 17px", borderRadius: 12, background: "#f6f3ee" }}><div style={{ fontSize: 17, color: muted }}>Beløp</div><div style={{ fontSize: 28, marginTop: 3 }}>24 800 kr</div></Enter>
					<Enter at={145} style={{ padding: "14px 17px", borderRadius: 12, background: "#f6f3ee" }}><div style={{ fontSize: 17, color: muted }}>Forfalt</div><div style={{ fontSize: 28, marginTop: 3 }}>10. september</div></Enter>
				</div>
				<Enter at={180} style={{ marginTop: 20 }}><div style={{ ...row, justifyContent: "space-between" }}><span style={{ fontSize: 20, color: muted }}>Kontrollert mot Visma</span><Chip done>Samme faktura</Chip></div></Enter>
			</Enter>
		</Sequence>
		<Sequence name="Arbeidet er utført" layout="none">
			<Enter at={300} style={{ ...panel, ...row, position: "absolute", left: 96, right: 44, bottom: 35, padding: "20px 24px", background: ink, color: paper, borderColor: "#ffffff18", boxShadow: "0 18px 50px #241b1938" }}>
				<span style={{ display: "grid", placeItems: "center", width: 43, height: 43, borderRadius: 12, background: "#ffffff16", color: "#ebaa8a" }}>{complete ? <Check size={23} /> : <FileText size={23} />}</span>
				<div style={{ flex: 1 }}><div style={{ fontSize: 23 }}>{frame >= 430 ? "Oppfølging sendt. Loggen er oppdatert." : complete ? "Oppfølgingen er klar." : "Skriver oppfølging …"}</div><div style={{ color: "#d6d1c9", fontSize: 17, marginTop: 4 }}>{complete ? "Innenfor rammene dere har satt." : "Med riktig beløp, tone og mottaker."}</div></div>
				{complete ? <ShieldCheck size={26} color="#ebaa8a" /> : null}
			</Enter>
		</Sequence>
	</Stage>;
}

function LearnScene() {
	const frame = useCurrentFrame();
	const recede = interpolate(frame, [80, 125], [0, 1], clamp);
	return <Stage kind="learn">
		{/* Beat one: the three existing cards establish the way the team works. */}
		<div style={{ opacity: 1 - recede * .28, transform: `translateX(${-recede * 24}px)` }}>
			<Sequence name="Slik lager teamet rapporten" layout="none">
				<Enter at={0} style={{ ...panel, position: "absolute", top: 44, left: 54, width: 566, padding: "21px 25px" }}>
					<div style={{ ...row, justifyContent: "space-between", paddingBottom: 14, borderBottom: `1px solid ${line}` }}><span style={row}><Brand small /><span style={{ fontSize: 22 }}>Verevon lærer av dere</span></span><span style={{ width: 9, height: 9, borderRadius: "50%", background: clay }} /></div>
					<div style={{ fontSize: 28, marginTop: 18, letterSpacing: "-.025em" }}>Slik lager vi ukesrapporten.</div>
					<div style={{ color: muted, fontSize: 19, marginTop: 5 }}>Salg → vurdering → neste steg</div>
					<div style={{ display: "flex", alignItems: "flex-end", gap: 14, height: 102, marginTop: 18, paddingBottom: 8, borderBottom: `1px solid ${line}` }}>
						{[.42, .62, .49, .83, .69, .96, .87].map((height, i) => <div key={i} style={{ flex: 1, borderRadius: "6px 6px 0 0", height: `${height * 100}%`, background: i === 5 ? clay : "#d9d5cb" }} />)}
					</div>
					<div style={{ ...row, justifyContent: "space-between", marginTop: 12, fontSize: 17, color: muted }}><span>Uke 37</span><span style={{ color: clay }}>Legg til vurderingen vår</span></div>
				</Enter>
			</Sequence>
			<Sequence name="Teamets tilbakemelding" layout="none">
				<Enter at={18} style={{ ...panel, ...row, position: "absolute", left: 32, top: 361, width: 503, padding: "20px 24px" }}><span style={{ fontSize: 17, padding: "7px 10px", background: "#ede6dc", borderRadius: "50%" }}>Du</span><span style={{ fontSize: 24 }}>«Løft frem avvikene først.»</span></Enter>
			</Sequence>
			<Sequence name="Verevon tar med seg arbeidsmåten" layout="none">
				<Enter at={36} style={{ ...panel, ...row, position: "absolute", top: 470, right: 34, width: 535, padding: "20px 24px", background: ink, color: paper }}><Sparkles size={27} color="#e9ab8b" /><div><div style={{ fontSize: 23 }}>Jeg husker det til neste gang.</div><div style={{ fontSize: 18, marginTop: 4, color: "#d2ccc5" }}>Arbeidsmåten deres er oppdatert.</div></div></Enter>
			</Sequence>
		</div>
		{/* Beat two: the outcome moves forward; the process remains visible behind it. */}
		<Sequence name="Rapporten er klar" layout="none">
			<Enter at={94} style={{ ...panel, position: "absolute", top: 142, left: 132, right: 24, bottom: 20, padding: "25px 29px", background: "#fffefa", boxShadow: "0 24px 90px #48301e40" }}>
				<div style={{ ...row, justifyContent: "space-between" }}><Brand small /><Chip done>Arbeidsmåte lært</Chip></div>
				<div style={{ fontSize: 17, color: muted, marginTop: 17 }}>Neste mandag · 08:00</div>
				<div style={{ fontSize: 35, lineHeight: 1.12, letterSpacing: "-.045em", marginTop: 8 }}>God morgen.<br />Rapporten er klar.</div>
				<div style={{ padding: "12px 0 15px", fontSize: 20, color: muted }}>Avvikene først. Slik dere viste meg.</div>
				<ResultRow at={122} label="Salgstall samlet" sub="Fra bedriftens systemer" />
				<ResultRow at={143} label="Avvik løftet frem" sub="Basert på tilbakemeldingen deres" />
				<div style={{ ...row, marginTop: 11, borderRadius: 12, padding: "13px 16px", background: "#f1e7de", color: clay }}><FileText size={22} /><span style={{ fontSize: 21 }}>Ukesrapport · uke 38</span><ArrowUpRight size={22} style={{ marginLeft: "auto" }} /></div>
			</Enter>
		</Sequence>
	</Stage>;
}

function OverseeScene() {
	const frame = useCurrentFrame();
	const modalSlide = interpolate(frame, [0, 62], [165, 0], clamp);
	return <Stage kind="oversee">
		<Sequence name="Én vurderingsflate" layout="none">
			<Enter at={0} style={{ position: "absolute", top: 57, left: 105, width: 682, height: 532 }}>
				<div style={{ ...panel, height: "100%", padding: "26px 28px", background: paper, borderRadius: "22px 0 0 22px", boxShadow: "-18px 18px 65px #31403d30", transform: `translateX(${modalSlide}px)` }}>
					<div style={{ ...row, width: 525, gap: 12, paddingBottom: 20, borderBottom: `1px solid ${line}` }}><Brand small /><span style={{ fontSize: 22 }}>Verevon</span><span style={{ marginLeft: "auto", fontSize: 16, color: clay }}>Til vurdering</span></div>
					<div style={{ marginTop: 24, fontSize: 17, letterSpacing: ".12em", textTransform: "uppercase", color: clay }}>Leverandøravtale</div>
					<div style={{ marginTop: 6, fontSize: 34, letterSpacing: "-.04em" }}>Dere bestemmer neste steg.</div>
					<div style={{ width: 525, marginTop: 8, fontSize: 19, color: muted }}>Vilkårene er gjennomgått. Verevon går ikke videre uten deres godkjenning.</div>
					<div style={{ width: 525, marginTop: 24, borderTop: `1px solid ${line}` }}>
						<div style={{ ...row, padding: "16px 0", borderBottom: `1px solid ${line}` }}><FileText size={21} color={muted} /><span style={{ fontSize: 19 }}>Avtalen er gjennomgått</span><Check size={20} color="#5c7661" style={{ marginLeft: "auto" }} /></div>
						<div style={{ ...row, padding: "16px 0", borderBottom: `1px solid ${line}` }}><ShieldCheck size={21} color={muted} /><span style={{ fontSize: 19 }}>Kari: «Dette ser bra ut.»</span><Check size={20} color="#5c7661" style={{ marginLeft: "auto" }} /></div>
					</div>
					<Enter at={74} style={{ width: 525, marginTop: 18, padding: "10px 0 10px 17px", borderLeft: `3px solid ${clay}` }}>
						<div style={{ ...row, gap: 9, color: clay, fontSize: 17 }}><ShieldCheck size={21} /> Menneskelig kontroll</div>
						<div style={{ fontSize: 27, letterSpacing: "-.035em", marginTop: 5 }}>Godkjenn neste steg</div>
						<div style={{ ...row, justifyContent: "space-between", marginTop: 5, fontSize: 17, color: muted }}><span>Ingenting sendes før dere godkjenner.</span><ArrowUpRight size={20} color={clay} /></div>
					</Enter>
				</div>
			</Enter>
		</Sequence>
	</Stage>;
}

export function SenseScene({ kind }: { kind: SenseKind }) {
	if (kind === "delegate") return <DelegateScene />;
	if (kind === "learn") return <LearnScene />;
	return <OverseeScene />;
}
