import { ImageResponse } from "next/og";

export const alt = "Verevon — Fra kundesignal til godkjent handling";
export const size = { width: 1200, height: 630 };
export const contentType = "image/png";

export default function OpenGraphImage() {
	return new ImageResponse(
		(
			<div
				style={{
					alignItems: "flex-start",
					background: "#f6f6f4",
					color: "#171717",
					display: "flex",
					flexDirection: "column",
					height: "100%",
					justifyContent: "space-between",
					padding: "72px",
					width: "100%",
				}}
			>
				<div
					style={{
						alignItems: "center",
						display: "flex",
						fontSize: 34,
						fontWeight: 700,
						letterSpacing: "0.16em",
					}}
				>
					VEREVON
				</div>
				<div style={{ display: "flex", flexDirection: "column", gap: "28px" }}>
					<div
						style={{
							color: "#d66a4d",
							fontSize: 24,
							fontWeight: 700,
							letterSpacing: "0.12em",
						}}
					>
						KUNNSKAP · KONTROLL · HANDLING
					</div>
					<div
						style={{
							fontSize: 78,
							fontWeight: 400,
							letterSpacing: "-0.06em",
							lineHeight: 0.94,
						}}
					>
						Fra kundesignal til godkjent handling.
					</div>
				</div>
				<div
					style={{
						alignItems: "center",
						display: "flex",
						fontSize: 28,
						gap: "14px",
						letterSpacing: "0.04em",
					}}
				>
					<div
						style={{
							background: "#d66a4d",
							height: "10px",
							width: "132px",
						}}
					/>
					Finn. Forstå. Få gjort.
				</div>
			</div>
		),
		size,
	);
}
