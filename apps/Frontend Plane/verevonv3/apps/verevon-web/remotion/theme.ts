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
		font-family: "Arbeit";
		font-style: normal;
		font-weight: 300;
		src: url("${staticFile("fonts/arbeit-pro-light/arbeit-pro-light.woff")}") format("woff");
	}
	@font-face {
		font-family: "Arbeit";
		font-style: normal;
		font-weight: 400;
		src: url("${staticFile("fonts/arbeit-pro-book/arbeit-pro-book.woff")}") format("woff");
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

export const arbeit: CSSProperties = {
	fontFamily: "Arbeit, Arial, sans-serif",
};

export const protokoll: CSSProperties = {
	fontFamily: "Protokoll, Arial, sans-serif",
};
