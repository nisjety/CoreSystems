import { betterAuth } from "better-auth";
import { nextCookies } from "better-auth/next-js";
import { twoFactor } from "better-auth/plugins/two-factor";
import { passkey } from "@better-auth/passkey";
import { createAuthDatabase } from "@/lib/auth/database";
import {
	authRuntimePlan,
	getAuthBaseUrl,
	getAuthSecret,
	getTrustedOrigins,
	isSecureCookieRuntime,
} from "@/lib/auth/runtime";

const database = createAuthDatabase();

export const auth = betterAuth({
	appName: "Verevon",
	baseURL: getAuthBaseUrl(),
	secret: getAuthSecret(),
	...(database ? { database } : {}),
	trustedOrigins: getTrustedOrigins(),
	advanced: {
		cookiePrefix: "verevon",
		useSecureCookies: isSecureCookieRuntime(),
	},
	emailAndPassword: {
		enabled: true,
		minPasswordLength: authRuntimePlan.passwordMinimumLength,
		requireEmailVerification: true,
	},
	session: {
		expiresIn: authRuntimePlan.sessionMaxAgeDays * 24 * 60 * 60,
		updateAge: authRuntimePlan.sessionRefreshHours * 60 * 60,
	},
	rateLimit: {
		enabled: true,
		window: 60,
		max: 100,
	},
	plugins: [
		twoFactor({
			issuer: "Verevon",
			twoFactorCookieMaxAge:
				authRuntimePlan.twoFactorChallengeMinutes * 60,
			trustDeviceMaxAge: authRuntimePlan.trustedDeviceDays * 24 * 60 * 60,
			otpOptions: {
				digits: 6,
				period: 3,
				allowedAttempts: 5,
				async sendOTP({ user, otp }) {
					if (
						process.env.NODE_ENV !== "production" &&
						process.env.VEREVON_AUTH_LOG_DEV_OTP === "1"
					) {
						console.info(
							`Verevon development 2FA OTP for ${user.email}: ${otp}`,
						);
					}
				},
			},
			backupCodeOptions: {
				amount: 10,
				length: 10,
			},
		}),
		passkey({
			rpID: process.env.PASSKEY_RP_ID || "localhost",
			rpName: process.env.PASSKEY_RP_NAME || "Verevon",
			origin: process.env.PASSKEY_ORIGIN || getAuthBaseUrl(),
		}),
		nextCookies(),
	],
});

export type VerevonAuth = typeof auth;
