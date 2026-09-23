import type { CSSProperties, ReactNode } from "react";
import { AbsoluteFill, Easing, interpolate, Sequence, staticFile, useCurrentFrame } from "remotion";
import { ArrowUpRight, Check, FileText, Mail, MousePointer2, Search, ShieldCheck, Sparkles } from "lucide-react";
import { VerevonMark } from "../sections/VerevonMark";

export type SenseKind = "delegate" | "learn" | "oversee";
export const SENSE_DURATION = 540;
export const SENSE_FPS = 30;
export const SENSE_STEPS = [0, 180, 360] as const;

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
	const dark = kind === "oversee";
	const loopVeil = interpolate(frame, [0, 12, 516, 539], [0, 1, 1, 0], clamp);
	return <AbsoluteFill style={{ fontFamily: "Arbeit, Arial, sans-serif", fontSize: 23, color: ink, fontWeight: 400, lineHeight: 1.3, fontVariantNumeric: "tabular-nums", background: "transparent", overflow: "hidden" }}>
		<style>{`@font-face{font-family:Arbeit;src:url('${staticFile("fonts/arbeit-pro-book/arbeit-pro-book.woff")}') format('woff');font-weight:400}@font-face{font-family:Protokoll;src:url('${staticFile("fonts/protokoll-medium/ProtokollMedium-Web.woff2")}') format('woff2');font-weight:500}`}</style>
		<AbsoluteFill style={{ background: dark ? "linear-gradient(145deg,#19252299,transparent 60%,#1c1b1a99)" : "linear-gradient(150deg,#ffffff26,transparent 70%,#c3916726)" }} />
		{/* A single signal travels around the working area, then resolves into the result. */}
		<svg viewBox="0 0 720 640" style={{ position: "absolute", inset: 0, width: "100%", height: "100%", opacity: dark ? .4 : .27 }} fill="none">
			<path d="M-20 488 H74 Q98 488 98 464 V95 Q98 69 124 69 H606 Q632 69 632 95 V568 Q632 590 654 590 H740" stroke={dark ? "#d9ded6" : clay} strokeWidth="1" />
			<path d="M-20 488 H74 Q98 488 98 464 V95 Q98 69 124 69 H606 Q632 69 632 95 V568 Q632 590 654 590 H740" stroke={dark ? "#efe7d3" : clay} strokeWidth="2.5" strokeDasharray="45 1700" strokeDashoffset={-frame * 3.2} />
		</svg>
		<AbsoluteFill style={{ opacity: loopVeil }}>{children}</AbsoluteFill>
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
	return <Stage kind="learn">
		<Sequence name="Vis hvordan dere jobber" layout="none">
			<Enter at={0} style={{ ...panel, position: "absolute", top: 74, left: 64, width: 588, padding: "24px 26px", transformOrigin: "center top" }}>
				<div style={{ ...row, justifyContent: "space-between", paddingBottom: 19, borderBottom: `1px solid ${line}` }}><span style={row}><Brand small /><span style={{ fontSize: 22 }}>Verevon lærer av dere</span></span><span style={{ width: 9, height: 9, borderRadius: "50%", background: clay }} /></div>
				<div style={{ fontSize: 28, marginTop: 24, letterSpacing: "-.025em" }}>Slik lager vi ukesrapporten.</div>
				<div style={{ color: muted, fontSize: 20, marginTop: 8 }}>Salg → vurdering → neste steg</div>
				<div style={{ display: "flex", alignItems: "flex-end", gap: 15, height: 144, marginTop: 25, paddingBottom: 12, borderBottom: `1px solid ${line}` }}>
					{[.42, .62, .49, .83, .69, .96, .87].map((height, i) => <div key={i} style={{ flex: 1, borderRadius: "6px 6px 0 0", height: `${height * 100}%`, background: i === 5 ? clay : "#d9d5cb", transformOrigin: "bottom", transform: `scaleY(${interpolate(frame, [32 + i * 4, 65 + i * 4], [.06, 1], clamp)})` }} />)}
				</div>
				<div style={{ ...row, justifyContent: "space-between", marginTop: 15, fontSize: 18, color: muted }}><span>Uke 37</span><span style={{ color: clay }}>Legg til vurderingen vår</span></div>
			</Enter>
			<Enter at={70} until={260} style={{ position: "absolute", left: 470, top: 404, color: ink }}><div style={{ transform: `translate(${interpolate(frame, [70, 112], [70, 0], clamp)}px,${interpolate(frame, [70, 112], [40, 0], clamp)}px)` }}><MousePointer2 size={31} fill={ink} stroke="white" strokeWidth={1.5} /><div style={{ marginLeft: 20, padding: "4px 10px", borderRadius: 20, color: "white", background: ink, fontSize: 16 }}>Du viser</div></div></Enter>
		</Sequence>
		<Sequence name="Tilbakemelding blir kunnskap" layout="none">
			<Enter at={150} until={355} style={{ ...panel, ...row, position: "absolute", left: 40, top: 385, width: 580, padding: "24px 26px", background: "#fffefa" }}><span style={{ fontSize: 18, padding: "7px 10px", background: "#ede6dc", borderRadius: "50%" }}>Du</span><span style={{ fontSize: 25 }}>«Løft frem avvikene først.»</span></Enter>
			<Enter at={260} until={355} style={{ ...panel, ...row, position: "absolute", top: 490, right: 48, width: 550, padding: "21px 25px", background: ink, color: paper }}><Sparkles size={27} color="#e9ab8b" /><div><div style={{ fontSize: 24 }}>Jeg husker det til neste gang.</div><div style={{ fontSize: 18, marginTop: 5, color: "#d2ccc5" }}>Arbeidsmåten deres er oppdatert.</div></div></Enter>
		</Sequence>
		<Sequence name="Neste gang er arbeidet klart" layout="none">
			<Enter at={360} style={{ position: "absolute", inset: 44, ...panel, padding: "28px 32px", background: "#fffefa", boxShadow: "0 24px 90px #48301e3d" }}>
				<div style={{ ...row, justifyContent: "space-between" }}><Brand /><Chip done>Arbeidsmåte lært</Chip></div>
				<div style={{ fontSize: 18, color: muted, marginTop: 24 }}>Neste mandag · 08:00</div>
				<div style={{ fontSize: 35, lineHeight: 1.12, letterSpacing: "-.045em", marginTop: 10 }}>God morgen.<br />Rapporten er klar.</div>
				<div style={{ padding: "19px 0 20px", fontSize: 22, color: muted }}>Avvikene først. Akkurat slik dere viste meg.</div>
				<ResultRow at={386} label="Salgstall samlet" sub="Fra bedriftens systemer" />
				<ResultRow at={402} label="Avvik løftet frem" sub="Basert på tilbakemeldingen deres" />
				<div style={{ ...row, marginTop: 14, borderRadius: 12, padding: "14px 19px", background: "#f1e7de", color: clay }}><FileText size={23} /><span style={{ fontSize: 22 }}>Ukesrapport · uke 38</span><ArrowUpRight size={24} style={{ marginLeft: "auto" }} /></div>
			</Enter>
		</Sequence>
	</Stage>;
}

function OverseeScene() {
	const frame = useCurrentFrame();
	const review = frame >= 180;
	const approved = frame >= 399;
	return <Stage kind="oversee">
		<Sequence name="Følg arbeidet" layout="none">
			<Enter at={0} style={{ position: "absolute", top: 61, left: 54, right: 54, color: paper }}>
				<div style={{ ...row, justifyContent: "space-between" }}><span style={{ ...row, fontSize: 25 }}><Brand /> Deres arbeidsflate</span><span style={{ ...row, gap: 0 }}>{["KA", "MS", "+2"].map((name, i) => <span key={name} style={{ display: "grid", placeItems: "center", width: 37, height: 37, borderRadius: "50%", border: "2px solid #303331", background: ["#eadbcb", "#aab5a4", "#555b55"][i], color: i === 2 ? paper : ink, fontSize: 13, marginLeft: -7 }}>{name}</span>)}</span></div>
			</Enter>
			<Enter at={22} style={{ ...panel, position: "absolute", top: 140, left: 53, width: 614, padding: "22px 28px", background: "#f5f4eeee", filter: review ? "blur(2px)" : undefined }}>
				<div style={{ ...row, justifyContent: "space-between", marginBottom: 20 }}><span style={{ fontSize: 22 }}>Arbeid som går fremover</span><span style={{ fontSize: 17, color: muted }}>3 oppgaver</span></div>
				<ResultRow at={30} label="Ukesrapport" sub="Samler tall og forklarer avvik" />
				<ResultRow at={84} label="Kundeoppfølging" sub="Gjør neste samtale klar" />
				<ResultRow at={135} label="Avtale gjennomgått" sub="Venter på teamets klarsignal" />
			</Enter>
		</Sequence>
		<Sequence name="Teamet vurderer" layout="none">
			<Enter at={180} style={{ ...panel, position: "absolute", left: 83, right: 35, top: 185, padding: "28px 28px", boxShadow: "0 24px 90px #0005" }}>
				<div style={{ ...row, justifyContent: "space-between" }}><span style={{ fontSize: 18, color: muted }}>Leverandøravtale</span><Chip done={approved}>{approved ? "Godkjent" : "Til vurdering"}</Chip></div>
				<div style={{ fontSize: 31, letterSpacing: "-.035em", marginTop: 22 }}>{approved ? "Klarsignal gitt." : "Ett valg trenger dere."}</div>
				<div style={{ fontSize: 23, color: muted, marginTop: 10, maxWidth: 470 }}>{approved ? <>Forslaget er godkjent.<br />Verevon følger opp neste steg.</> : <>Vilkårene er gjennomgått.<br />Skal vi gå videre med forslaget?</>}</div>
				<div style={{ ...row, padding: "18px 0", marginTop: 20, borderTop: `1px solid ${line}`, borderBottom: `1px solid ${line}` }}><span style={{ display: "grid", placeItems: "center", borderRadius: "50%", background: "#e7ddd0", width: 35, height: 35, fontSize: 13 }}>KA</span><div style={{ fontSize: 19 }}>Kari: «Dette ser bra ut.»</div><Check size={20} style={{ marginLeft: "auto", color: muted }} /></div>
				<div style={{ ...row, justifyContent: "space-between", marginTop: 22 }}><span style={{ fontSize: 18, color: muted }}>{approved ? "Godkjent av deg og Kari" : "Dere bestemmer neste steg"}</span><span style={{ ...row, gap: 8, background: approved ? "#e8ede6" : ink, color: approved ? "#426049" : paper, padding: "13px 19px", borderRadius: 10, fontSize: 20, transform: `scale(${interpolate(frame, [383, 390, 399], [1, .96, 1], clamp)})` }}>{approved ? <Check size={19} /> : null}{approved ? "Godkjent" : "Godkjenn"}</span></div>
			</Enter>
			<Enter at={335} until={420} style={{ position: "absolute", top: 510, left: 544 }}><div style={{ transform: `translate(${interpolate(frame, [335, 381], [90, 0], clamp)}px,${interpolate(frame, [335, 381], [45, 0], clamp)}px)` }}><MousePointer2 size={32} fill="#fffefa" color={ink} /><span style={{ background: "#f7e9db", color: ink, padding: "5px 9px", marginLeft: 24, fontSize: 15, borderRadius: 20 }}>Du</span></div></Enter>
		</Sequence>
		<Sequence name="Verevon går videre" layout="none">
			<Enter at={423} style={{ ...row, position: "absolute", bottom: 20, left: 120, color: paper, gap: 10, fontSize: 22 }}><ShieldCheck size={24} color="#e9b395" /> Dere ga klarsignal. Verevon går videre.</Enter>
		</Sequence>
	</Stage>;
}

export function SenseScene({ kind }: { kind: SenseKind }) {
	if (kind === "delegate") return <DelegateScene />;
	if (kind === "learn") return <LearnScene />;
	return <OverseeScene />;
}
