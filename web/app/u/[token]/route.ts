import { NextRequest, NextResponse } from "next/server";
import { BRAND_NAME, getSiteUrl } from "../../../lib/config";
import { hashIpAddress, recordUnsubscribeEvent } from "../../../lib/db";
import { escapeHtml, getRequestIp, jsonError } from "../../../lib/http";
import { verifyUnsubscribeToken } from "../../../lib/unsubscribe-token";

export const dynamic = "force-dynamic";
export const runtime = "nodejs";

type RouteContext = {
  params: Promise<{
    token: string;
  }>;
};

function htmlPage({
  title,
  message,
  status = 200,
  tone = "success",
}: {
  title: string;
  message: string;
  status?: number;
  tone?: "success" | "error";
}): NextResponse {
  const safeTitle = escapeHtml(title);
  const safeMessage = escapeHtml(message);
  const safeBrand = escapeHtml(BRAND_NAME);
  const safeSite = escapeHtml(getSiteUrl());
  const safeTone = tone === "error" ? "error" : "success";

  return new NextResponse(
    `<!doctype html>
<html lang="en">
  <head>
    <meta charset="utf-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1" />
    <title>${safeTitle} | ${safeBrand}</title>
    <style>
      :root {
        color-scheme: light;
        --ink: #181713;
        --paper: #fffaf0;
        --field: #f1eadb;
        --line: #d9d0bf;
        --muted: #6e685d;
        --accent: #f3c83a;
        --danger: #9f1239;
      }
      * {
        box-sizing: border-box;
      }
      body {
        margin: 0;
        min-height: 100dvh;
        display: grid;
        place-items: center;
        background: var(--paper);
        color: var(--ink);
        font-family: ui-sans-serif, -apple-system, BlinkMacSystemFont, "Avenir Next", "Segoe UI", sans-serif;
        padding: 24px;
      }
      main {
        width: min(620px, 100%);
        background: var(--field);
        border: 1px solid var(--line);
        padding: 28px;
        box-shadow: 14px 14px 0 var(--accent);
      }
      .top {
        display: flex;
        align-items: center;
        justify-content: space-between;
        gap: 16px;
        border-bottom: 1px solid var(--line);
        padding-bottom: 22px;
      }
      .brand {
        display: flex;
        align-items: center;
        gap: 12px;
      }
      .mark {
        width: 44px;
        height: 44px;
      }
      .brand strong,
      .pill {
        font-size: 13px;
        font-weight: 800;
      }
      .brand span {
        display: block;
        margin-top: 2px;
        color: var(--muted);
        font-size: 13px;
      }
      .pill {
        background: ${safeTone === "success" ? "var(--accent)" : "var(--danger)"};
        color: ${safeTone === "success" ? "var(--ink)" : "var(--paper)"};
        padding: 8px 10px;
      }
      h1 {
        max-width: 520px;
        margin: 46px 0 0;
        font-size: 44px;
        line-height: 1;
        letter-spacing: 0;
        text-wrap: balance;
      }
      p {
        max-width: 530px;
        color: var(--muted);
        font-size: 17px;
        line-height: 1.6;
        margin: 18px 0 0;
      }
      a {
        display: inline-flex;
        align-items: center;
        justify-content: center;
        margin-top: 28px;
        min-height: 46px;
        border: 1px solid var(--ink);
        background: var(--ink);
        color: var(--paper);
        padding: 12px 16px;
        text-decoration: none;
        font-weight: 800;
      }
      a:hover,
      a:focus-visible {
        background: var(--accent);
        border-color: var(--accent);
        color: var(--ink);
        outline: none;
      }
      @media (max-width: 560px) {
        body {
          place-items: start center;
        }
        main {
          padding: 22px;
          box-shadow: 8px 8px 0 var(--accent);
        }
        .top {
          align-items: flex-start;
          flex-direction: column;
        }
        h1 {
          font-size: 34px;
        }
      }
    </style>
  </head>
  <body>
    <main>
      <div class="top">
        <div class="brand">
          <img class="mark" src="/sharpclap-mark.svg" alt="" />
          <div>
            <strong>${safeBrand}</strong>
            <span>mailing list preference center</span>
          </div>
        </div>
        <span class="pill">${safeTone === "success" ? "Recorded" : "Check link"}</span>
      </div>
      <h1>${safeTitle}</h1>
      <p>${safeMessage}</p>
      <a href="${safeSite}">Return to ${safeBrand}</a>
    </main>
  </body>
</html>`,
    {
      status,
      headers: {
        "Content-Type": "text/html; charset=utf-8",
        "Cache-Control": "no-store",
      },
    },
  );
}

async function record(request: NextRequest, token: string, method: "GET" | "POST") {
  const verified = verifyUnsubscribeToken(token);
  return recordUnsubscribeEvent({
    verified,
    method,
    userAgent: request.headers.get("user-agent"),
    ipHash: hashIpAddress(getRequestIp(request)),
  });
}

export async function GET(request: NextRequest, context: RouteContext) {
  const { token } = await context.params;

  try {
    await record(request, token, "GET");
    return htmlPage({
      title: "You are off the list.",
      message:
        "The unsubscribe request has been recorded. Future campaigns using this list will skip this address.",
    });
  } catch (error) {
    if (error instanceof Error && error.name === "TokenVerificationError") {
      return htmlPage({
        title: "Invalid unsubscribe link",
        message: "This link is not valid anymore. The campaign owner can send a fresh preference link if needed.",
        status: 400,
        tone: "error",
      });
    }
    return jsonError(error);
  }
}

export async function POST(request: NextRequest, context: RouteContext) {
  const { token } = await context.params;

  try {
    const event = await record(request, token, "POST");
    return NextResponse.json({
      status: "success",
      data: {
        id: event.id,
        recorded: true,
      },
    });
  } catch (error) {
    return jsonError(error);
  }
}
