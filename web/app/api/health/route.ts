import { NextResponse } from "next/server";
import { BRAND_NAME, getMissingRuntimeConfig, getSiteUrl } from "../../../lib/config";
import { jsonError } from "../../../lib/http";
import { pingDatabase } from "../../../lib/db";

export const dynamic = "force-dynamic";
export const runtime = "nodejs";

export async function GET() {
  const missing = getMissingRuntimeConfig();
  let database = "not_checked";

  try {
    if (process.env.DATABASE_URL) {
      await pingDatabase();
      database = "ok";
    } else {
      database = "missing";
    }

    const ready = missing.length === 0 && database === "ok";
    return NextResponse.json(
      {
        status: ready ? "success" : "error",
        data: {
          service: BRAND_NAME,
          ready,
          site_url: getSiteUrl(),
          database,
          missing,
          endpoints: {
            unsubscribe: "/u/:token",
            sync: "/api/unsubscribes?after=0",
          },
        },
      },
      { status: ready ? 200 : 503 },
    );
  } catch (error) {
    return jsonError(error);
  }
}
