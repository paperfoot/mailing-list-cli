import { createHmac, timingSafeEqual } from "crypto";
import { requireUnsubscribeSecret } from "./config";

export type VerifiedUnsubscribeToken = {
  token: string;
  contactId: number;
  broadcastId: number;
  issuedAt: number;
  payload: string;
};

export class TokenVerificationError extends Error {
  constructor(message: string) {
    super(message);
    this.name = "TokenVerificationError";
  }
}

function decodeBase64Url(value: string): Buffer {
  if (!/^[A-Za-z0-9_-]+$/.test(value)) {
    throw new TokenVerificationError("token contains invalid base64url data");
  }

  const padded = value.padEnd(value.length + ((4 - (value.length % 4)) % 4), "=");
  return Buffer.from(padded.replaceAll("-", "+").replaceAll("_", "/"), "base64");
}

function encodeBase64Url(value: Buffer): string {
  return value.toString("base64").replaceAll("+", "-").replaceAll("/", "_").replaceAll("=", "");
}

function parseSafeInteger(value: string, label: string): number {
  if (!/^\d+$/.test(value)) {
    throw new TokenVerificationError(`${label} is not an integer`);
  }

  const parsed = Number(value);
  if (!Number.isSafeInteger(parsed) || parsed < 0) {
    throw new TokenVerificationError(`${label} is outside the safe integer range`);
  }

  return parsed;
}

export function verifyUnsubscribeToken(token: string): VerifiedUnsubscribeToken {
  const [payloadB64, signatureB64, ...extra] = token.split(".");
  if (!payloadB64 || !signatureB64 || extra.length > 0) {
    throw new TokenVerificationError("invalid token format");
  }

  const payloadBytes = decodeBase64Url(payloadB64);
  const signatureBytes = decodeBase64Url(signatureB64);
  const expectedSignature = createHmac("sha256", requireUnsubscribeSecret())
    .update(payloadBytes)
    .digest();

  if (
    signatureBytes.length !== expectedSignature.length ||
    !timingSafeEqual(signatureBytes, expectedSignature)
  ) {
    throw new TokenVerificationError("signature mismatch");
  }

  if (encodeBase64Url(expectedSignature) !== signatureB64) {
    throw new TokenVerificationError("non-canonical signature encoding");
  }

  const payload = payloadBytes.toString("utf8");
  const parts = payload.split(":");
  if (parts.length !== 3) {
    throw new TokenVerificationError("invalid token payload");
  }

  return {
    token,
    contactId: parseSafeInteger(parts[0], "contact_id"),
    broadcastId: parseSafeInteger(parts[1], "broadcast_id"),
    issuedAt: parseSafeInteger(parts[2], "issued_at"),
    payload,
  };
}
