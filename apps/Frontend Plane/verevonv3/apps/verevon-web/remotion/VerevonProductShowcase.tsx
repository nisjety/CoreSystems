import { AbsoluteFill, Img, interpolate, staticFile, useCurrentFrame } from "remotion";
import { FONT_CSS } from "./theme";

/** A real, unsent SPA prompt. The labels describe the workflow, not fabricated results. */
export function VerevonProductShowcase() {
  const frame = useCurrentFrame();
  return (
    <AbsoluteFill style={{ background: "#e9e5df", fontFamily: "Arbeit, sans-serif", color: "#171717" }}>
      <style>{FONT_CSS}</style>
      <Img src={staticFile("feature-film-backgrounds/connect-systems-pastel.jpg")} style={{ width: "100%", height: "100%", objectFit: "cover", opacity: 0.48, filter: "saturate(0.5)", scale: interpolate(frame, [0, 239], [1.04, 1.08]) }} />
      <AbsoluteFill style={{ justifyContent: "center", alignItems: "center", padding: 88 }}>
        <div style={{ fontFamily: "Protokoll, sans-serif", fontSize: 15, letterSpacing: "0.16em", textTransform: "uppercase", color: "#665d52", marginBottom: 24 }}>Verevon / Fra spørsmål til neste steg</div>
        <div style={{ fontSize: 58, fontWeight: 300, letterSpacing: "-0.065em", lineHeight: 0.96, textAlign: "center", marginBottom: 52 }}>Ett sted å starte.<br />Et grunnlag å gå videre med.</div>
        <div style={{ width: 1000, borderRadius: 24, overflow: "hidden", boxShadow: "0 32px 80px rgba(35,31,25,0.14)", translate: `0 ${interpolate(frame, [0, 32], [16, 0], { extrapolateRight: "clamp" })}px`, opacity: interpolate(frame, [0, 24], [0.85, 1], { extrapolateRight: "clamp" }) }}>
          <Img src={staticFile("verevon-product-shots/composer-norwegian-live.png")} style={{ display: "block", width: "100%" }} />
        </div>
        <div style={{ display: "flex", alignItems: "center", gap: 28, marginTop: 40, fontFamily: "Protokoll, sans-serif", fontSize: 19, color: "#4e4943" }}>
          {["Still spørsmålet", "Se grunnlaget", "Velg neste steg"].map((label, index) => <div key={label} style={{ opacity: interpolate(frame, [20 + index * 22, 42 + index * 22], [0.3, 1], { extrapolateLeft: "clamp", extrapolateRight: "clamp" }) }}><span style={{ color: "#a45a34", marginRight: 12 }}>0{index + 1}</span>{label}</div>)}
        </div>
      </AbsoluteFill>
    </AbsoluteFill>
  );
}
