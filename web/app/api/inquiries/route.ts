import { NextRequest, NextResponse } from "next/server";
import { recordInquiry } from "../../../lib/db";
import { jsonError } from "../../../lib/http";

export const dynamic = "force-dynamic";
export const runtime = "nodejs";

type InquiryBody = {
  name?: unknown;
  replyContact?: unknown;
  organization?: unknown;
  listSize?: unknown;
  agentStack?: unknown;
  message?: unknown;
  website?: unknown;
};

function clean(value: unknown, maxLength: number): string {
  if (typeof value !== "string") return "";
  return value.trim().replace(/\s+/g, " ").slice(0, maxLength);
}

function cleanLong(value: unknown, maxLength: number): string {
  if (typeof value !== "string") return "";
  return value.trim().slice(0, maxLength);
}

export async function POST(request: NextRequest) {
  try {
    const body = (await request.json().catch(() => null)) as InquiryBody | null;
    if (!body || typeof body !== "object") {
      return NextResponse.json(
        {
          status: "error",
          error: {
            code: "invalid_inquiry_payload",
            message: "Send a JSON inquiry payload.",
          },
        },
        { status: 400 },
      );
    }

    if (clean(body.website, 200)) {
      return NextResponse.json({
        status: "success",
        data: {
          recorded: true,
        },
      });
    }

    const name = clean(body.name, 140);
    const replyContact = clean(body.replyContact, 220);
    const organization = clean(body.organization, 180) || null;
    const listSize = clean(body.listSize, 80) || null;
    const agentStack = clean(body.agentStack, 220) || null;
    const message = cleanLong(body.message, 4000);

    if (!name || !replyContact || !message) {
      return NextResponse.json(
        {
          status: "error",
          error: {
            code: "missing_required_fields",
            message: "Name, reply contact, and message are required.",
          },
        },
        { status: 400 },
      );
    }

    const inquiry = await recordInquiry({
      name,
      replyContact,
      organization,
      listSize,
      agentStack,
      message,
      source: "sharpclap_home",
      userAgent: request.headers.get("user-agent"),
    });

    return NextResponse.json({
      status: "success",
      data: {
        id: inquiry.id,
        recorded: true,
      },
    });
  } catch (error) {
    return jsonError(error);
  }
}
