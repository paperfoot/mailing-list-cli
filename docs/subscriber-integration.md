# Subscriber Integration Guide

`mailing-list-cli` is a local CLI — it doesn't expose HTTP endpoints for signup forms. **That's by design.** The CLI manages your list; your website/backend calls the CLI to add subscribers.

This doc shows how to wire a signup form to the CLI from common web stacks.

## The command

```bash
mailing-list-cli contact add <email> --list <list-id> \
  --first-name "Alice" --last-name "Smith"
```

`--list` takes the numeric list id, not the list name. Find it once with `mailing-list-cli list ls`; the examples below use `1`.

That's it. The CLI handles:
- Deduplication (same email = no-op)
- Suppression checking (won't re-add bounced/complained/erased contacts)
- Syncing to Resend via email-cli

## Next.js API Route (App Router)

```typescript
// app/api/subscribe/route.ts
import { NextResponse } from "next/server";
import { execSync } from "child_process";

export async function POST(req: Request) {
  const { email, firstName, lastName } = await req.json();

  if (!email || !email.includes("@")) {
    return NextResponse.json({ error: "Invalid email" }, { status: 400 });
  }

  try {
    const args = [
      "contact", "add", email,
      "--list", "1",  // your numeric list id (find it: mailing-list-cli list ls)
    ];
    if (firstName) args.push("--first-name", firstName);
    if (lastName) args.push("--last-name", lastName);

    const result = execSync(
      `mailing-list-cli ${args.map(a => `"${a}"`).join(" ")}`,
      { encoding: "utf-8", timeout: 10000 }
    );

    return NextResponse.json({ ok: true });
  } catch (err: any) {
    const exitCode = err.status;
    if (exitCode === 3) {
      // Bad input (e.g., invalid email format)
      return NextResponse.json({ error: "Invalid input" }, { status: 400 });
    }
    return NextResponse.json({ error: "Server error" }, { status: 500 });
  }
}
```

## Express.js

```javascript
const { execSync } = require("child_process");

app.post("/subscribe", (req, res) => {
  const { email, firstName } = req.body;
  try {
    execSync(
      `mailing-list-cli contact add "${email}" --list 1` +
      (firstName ? ` --first-name "${firstName}"` : ""),
      { timeout: 10000 }
    );
    res.json({ ok: true });
  } catch (err) {
    res.status(err.status === 3 ? 400 : 500).json({ error: "Failed" });
  }
});
```

## Python (Flask / FastAPI)

```python
import subprocess

@app.post("/subscribe")
def subscribe(email: str, first_name: str = ""):
    cmd = ["mailing-list-cli", "contact", "add", email, "--list", "1"]
    if first_name:
        cmd += ["--first-name", first_name]
    result = subprocess.run(cmd, capture_output=True, timeout=10)
    if result.returncode == 0:
        return {"ok": True}
    elif result.returncode == 3:
        return {"error": "Invalid input"}, 400
    else:
        return {"error": "Server error"}, 500
```

## CSV Bulk Import

For migrating from another platform:

```bash
mailing-list-cli contact import subscribers.csv --list 1
```

The CSV needs `email` and `consent_source` columns. Imports without a populated `consent_source` on every row are rejected (`csv_missing_consent_source` / `csv_row_missing_consent`) unless you pass `--unsafe-no-consent`, which tags every imported row `imported_without_consent`. Optional columns: `first_name`, `last_name`, `tags` (comma-separated), and any custom fields you've created with `field create`.

## Exit Codes

Your integration code should handle these:

| Code | Meaning | Action |
|------|---------|--------|
| 0 | Success | Subscriber added |
| 1 | Transient error | Retry after a delay |
| 2 | Config error | Check server setup (is mailing-list-cli installed? Is email-cli configured?) |
| 3 | Bad input | Invalid email or missing required fields — surface to user |
| 4 | Rate limited | Back off and retry |

## Double Opt-In

`mailing-list-cli` doesn't send confirmation emails itself (that requires a hosted endpoint to handle the click). Implement double opt-in in your backend:

1. User submits signup form
2. Your backend sends a confirmation email (via email-cli or any transactional sender)
3. User clicks the confirmation link → hits your backend
4. Backend calls `mailing-list-cli contact add <email> --list 1`

Only step 4 touches the CLI. Steps 1-3 are your standard web flow.

## Agent-Driven Subscriber Management

If you're using an AI agent to manage the list, the agent drives the CLI directly:

```bash
# Add a subscriber
mailing-list-cli contact add alice@example.com --list 1 --first-name Alice

# Tag them
mailing-list-cli contact tag alice@example.com vip

# Set a custom field
mailing-list-cli contact set alice@example.com company "Acme Corp"

# Check their profile
mailing-list-cli contact show alice@example.com
```

The agent uses `agent-info` to discover all available commands at runtime — no hardcoded docs needed.
