import { NextRequest, NextResponse } from "next/server";
import { listUnsubscribeEvents } from "../../../lib/db";
import { jsonError } from "../../../lib/http";
import { requireSyncApiKey } from "../../../lib/config";

export const dynamic = "force-dynamic";
export const runtime = "nodejs";

function parseCursor(value: string | null): number {
  if (!value) return 0;
  const parsed = Number(value);
  return Number.isSafeInteger(parsed) && parsed >= 0 ? parsed : 0;
}

function parseLimit(value: string | null): number {
  if (!value) return 100;
  const parsed = Number(value);
  if (!Number.isSafeInteger(parsed) || parsed < 1) return 100;
  return Math.min(parsed, 500);
}

function isAuthorized(request: NextRequest): boolean {
  const expected = requireSyncApiKey();
  const auth = request.headers.get("authorization") ?? "";
  const bearer = auth.match(/^Bearer\s+(.+)$/i)?.[1];
  const header = request.headers.get("x-sync-api-key");
  return bearer === expected || header === expected;
}

export async function GET(request: NextRequest) {
  try {
    if (!isAuthorized(request)) {
      return NextResponse.json(
        {
          status: "error",
          error: {
            code: "unauthorized",
            message: "Missing or invalid sync API key",
          },
        },
        { status: 401 },
      );
    }

    const after = parseCursor(request.nextUrl.searchParams.get("after"));
    const limit = parseLimit(request.nextUrl.searchParams.get("limit"));
    const rows = await listUnsubscribeEvents({ after, limit });
    const nextCursor = rows.length > 0 ? rows[rows.length - 1].id : after;

    return NextResponse.json({
      status: "success",
      data: {
        events: rows.map((row) => ({
          id: row.id,
          token: row.token,
          contact_id: row.contact_id,
          broadcast_id: row.broadcast_id,
          issued_at: row.issued_at,
          method: row.method,
          last_method: row.last_method,
          seen_count: row.seen_count,
          created_at: row.created_at,
          last_seen_at: row.last_seen_at,
        })),
        next_cursor: nextCursor,
        has_more: rows.length === limit,
      },
    });
  } catch (error) {
    return jsonError(error);
  }
}
