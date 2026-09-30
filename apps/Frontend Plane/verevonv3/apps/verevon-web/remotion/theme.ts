import type { CSSProperties } from "react";
import { staticFile } from "remotion";

export const COLORS = {
	background: "#f5f4f0",
	coral: "#ee7a50",
	coralDeep: "#793819",
	coralSoft: "#fff1e7",
	ink: "#171717",
	muted: "#6d6963",
	line: "rgba(23, 23, 23, 0.11)",
	lineStrong: "rgba(23, 23, 23, 0.18)",
	paper: "#fffefa",
	white: "#ffffff",
	success: "#36755a",
	successSoft: "#eaf3ed",
};

export const FONT_CSS = `
	@font-face {
	font-family: Manrope;
	font-style: normal;
	font-weight: 200 800;
	font-display: swap;
	src: url("${staticFile("fonts/manrope/manrope-latin-ext-variable.woff2")}") format("woff2");
	unicode-range: U+0100-02BA, U+02BD-02C5, U+02C7-02CC, U+02CE-02D7, U+02DD-02FF, U+0304, U+0308, U+0329, U+1D00-1DBF, U+1E00-1E9F, U+1EF2-1EFF, U+2020, U+20A0-20AB, U+20AD-20C0, U+2113, U+2C60-2C7F, U+A720-A7FF;
}
	@font-face {
	font-family: Manrope;
	font-style: normal;
	font-weight: 200 800;
	font-display: swap;
	src: url("${staticFile("fonts/manrope/manrope-latin-variable.woff2")}") format("woff2");
	unicode-range: U+0000-00FF, U+0131, U+0152-0153, U+02BB-02BC, U+02C6, U+02DA, U+02DC, U+0304, U+0308, U+0329, U+2000-206F, U+20AC, U+2122, U+2191, U+2193, U+2212, U+2215, U+FEFF, U+FFFD;
}
	@font-face {
		font-family: "Protokoll";
		font-style: normal;
		font-weight: 300;
		src: url("${staticFile("fonts/protokoll-light/ProtokollLight-Web.woff2")}") format("woff2");
	}
	@font-face {
		font-family: "Protokoll";
		font-style: normal;
		font-weight: 500;
		src: url("${staticFile("fonts/protokoll-medium/ProtokollMedium-Web.woff2")}") format("woff2");
	}
`;

export const surfaceStyle: CSSProperties = {
	background: "rgba(255, 254, 250, 0.88)",
	border: "1px solid rgba(255, 255, 255, 0.92)",
	boxShadow:
		"0 34px 90px rgba(34, 31, 28, 0.11), inset 0 1px 0 rgba(255, 255, 255, 0.95)",
};

export const cardStyle: CSSProperties = {
	background: "rgba(255, 254, 250, 0.98)",
	border: `1px solid ${COLORS.lineStrong}`,
	boxShadow: "0 18px 42px rgba(37, 33, 29, 0.075)",
};

export const manrope: CSSProperties = {
	fontFamily: "Manrope, Arial, sans-serif",
};

export const protokoll: CSSProperties = {
	fontFamily: "Protokoll, Arial, sans-serif",
};
