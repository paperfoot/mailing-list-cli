import { NextRequest, NextResponse } from "next/server";
import { ConfigError } from "./config";
import { TokenVerificationError } from "./unsubscribe-token";

export function getRequestIp(request: NextRequest): string | null {
  const forwardedFor = request.headers.get("x-forwarded-for");
  if (forwardedFor) {
    return forwardedFor.split(",")[0]?.trim() || null;
  }
  return request.headers.get("x-real-ip");
}

export function jsonError(error: unknown, fallbackStatus = 500): NextResponse {
  if (error instanceof ConfigError) {
    return NextResponse.json(
      {
        status: "error",
        error: {
          code: "missing_runtime_config",
          message: error.message,
          missing: error.missing,
        },
      },
      { status: 503 },
    );
  }

  if (error instanceof TokenVerificationError) {
    return NextResponse.json(
      {
        status: "error",
        error: {
          code: "invalid_unsubscribe_token",
          message: error.message,
        },
      },
      { status: 400 },
    );
  }

  return NextResponse.json(
    {
      status: "error",
      error: {
        code: "internal_error",
        message: error instanceof Error ? error.message : "Unexpected server error",
      },
    },
    { status: fallbackStatus },
  );
}

export function escapeHtml(value: string): string {
  return value
    .replaceAll("&", "&amp;")
    .replaceAll("<", "&lt;")
    .replaceAll(">", "&gt;")
    .replaceAll('"', "&quot;")
    .replaceAll("'", "&#39;");
}
