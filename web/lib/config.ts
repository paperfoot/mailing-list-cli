export const BRAND_NAME = "SharpClap";
export const DEFAULT_SITE_URL = "https://sharpclap.com";

export class ConfigError extends Error {
  constructor(
    message: string,
    public readonly missing: string[],
  ) {
    super(message);
    this.name = "ConfigError";
  }
}

export function getSiteUrl(): string {
  return process.env.NEXT_PUBLIC_SITE_URL || DEFAULT_SITE_URL;
}

export function getMissingRuntimeConfig(): string[] {
  return ["DATABASE_URL", "MLC_UNSUBSCRIBE_SECRET", "SYNC_API_KEY"].filter(
    (key) => !process.env[key],
  );
}

export function requireDatabaseUrl(): string {
  const value = process.env.DATABASE_URL;
  if (!value) {
    throw new ConfigError("DATABASE_URL is not configured", ["DATABASE_URL"]);
  }
  return value;
}

export function requireUnsubscribeSecret(): string {
  const value = process.env.MLC_UNSUBSCRIBE_SECRET;
  if (!value) {
    throw new ConfigError("MLC_UNSUBSCRIBE_SECRET is not configured", [
      "MLC_UNSUBSCRIBE_SECRET",
    ]);
  }
  return value;
}

export function requireSyncApiKey(): string {
  const value = process.env.SYNC_API_KEY;
  if (!value) {
    throw new ConfigError("SYNC_API_KEY is not configured", ["SYNC_API_KEY"]);
  }
  return value;
}
