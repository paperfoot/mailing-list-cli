---
name: mailing-list-cli
description: "Use when user asks to manage mailing lists, send newsletters, manage contacts/subscribers, create email broadcasts, manage email templates, check email deliverability, import contacts, tag/segment subscribers, track revenue from emails, or any newsletter/mailing list management task. Triggers on 'newsletter', 'broadcast', 'mailing list', 'subscribers', 'contacts', 'email campaign', 'send email to list', 'unsubscribe', 'email template', 'deliverability', 'engagement report'."
---

# mailing-list-cli - Newsletter & Mailing List Management

Built on top of `email-cli`. Requires `email-cli >= 0.6.0` on PATH.

Current stable: `mailing-list-cli v0.4.3`.

Run `mailing-list-cli agent-info` for the source-of-truth capability manifest.

## Core Workflows

### Managing Lists & Contacts

```bash
mailing-list-cli list create "Newsletter" --description "Weekly updates"
mailing-list-cli list ls                                    # All lists with counts
mailing-list-cli contact add user@example.com --list 1 --first-name Jane
mailing-list-cli contact import contacts.csv --list 1       # Bulk CSV import (5 req/sec)
mailing-list-cli contact show user@example.com              # Full details
mailing-list-cli contact tag user@example.com "vip"         # Apply tag
```

### Tags & Segments

```bash
mailing-list-cli tag ls                                     # All tags with counts
mailing-list-cli segment create "Active VIPs" --filter-json '{"kind":"and","children":[{"kind":"atom","atom":{"type":"tag","pred":{"kind":"has","name":"vip"}}},{"kind":"atom","atom":{"type":"engagement","atom":{"kind":"opened_last","duration":{"value":30,"unit":"days"}}}}]}'
mailing-list-cli segment members "Active VIPs"              # Preview matching contacts
```

### Custom Fields

```bash
mailing-list-cli field create "company" --type text
mailing-list-cli field create "plan" --type select --options free,pro,enterprise
mailing-list-cli contact set user@example.com company "Acme Inc"
```

### Templates

```bash
mailing-list-cli template create "weekly-update" --from-file template.html
mailing-list-cli template lint "weekly-update"              # 6 lint rules
mailing-list-cli template preview "weekly-update" --open    # Render + open in browser
mailing-list-cli template render "weekly-update" --with-data vars.json | jq -e '.status == "success" and .data.lint_errors == 0' >/dev/null
mailing-list-cli template render "weekly-update" --with-data vars.json | jq -r '.data.html'
```

`template render` emits the full CLI JSON envelope, not raw HTML. Do not pass
its whole stdout to `email-cli --html`; use `template preview` for rendered
files and `broadcast preview` for a real inbox test.

Rendered plain-text alternatives preserve links as `Label (URL)`. Normal CTA
links receive campaign UTM tags automatically. Links with `data-utm="off"` are
not rewritten; generated unsubscribe body links already include this attribute.

`template lint` warns on unstyled `<a href>` links and fragile semantic layout
tags such as `<main>`, because email clients do not behave like full browsers.

### Broadcasts (Campaigns)

```bash
# 1. Create draft
mailing-list-cli broadcast create --name "April Update" --template weekly-update --to list:Newsletter

# 2. Preview test
mailing-list-cli broadcast preview <id> --to test@example.com

# 3. Schedule or send
mailing-list-cli broadcast schedule <id> --at "2026-04-15T09:00:00Z"
mailing-list-cli broadcast send <id> --dry-run              # Projected counts only
mailing-list-cli broadcast send <id> --confirm              # Send now (resumable, locked)
```

### Analytics & Reports

```bash
mailing-list-cli report show <broadcast-id>                 # Delivered/bounced/opened/clicked/CTR
mailing-list-cli report links <broadcast-id>                # Per-link click counts
mailing-list-cli report engagement --list Newsletter --days 30
mailing-list-cli report deliverability --days 30            # Bounce/complaint rates
```

Click counts come from `event poll`. `report links` can show CTA URLs when
`email-cli email list` includes `click.link` or `link`; if upstream only gives
`last_event=clicked`, aggregate `clicked_count` updates but link rows are empty.

### Revenue Tracking

```bash
mailing-list-cli revenue add <broadcast-id> --amount 99.00 --currency USD --email buyer@example.com
mailing-list-cli revenue import --from-stripe-csv export.csv
mailing-list-cli report revenue --days 90
mailing-list-cli report ltv                                 # Lifetime value
```

### Webhook Events

```bash
mailing-list-cli webhook poll                               # Sync delivery status from email-cli
mailing-list-cli event poll --reset                         # Reset + re-poll
```

### Health Check

```bash
mailing-list-cli health                                     # Config, DB, email-cli, schema, sender domain
```

### Updating

```bash
cargo install mailing-list-cli --force
brew update && brew upgrade mailing-list-cli
mailing-list-cli skill install
```

There are no `uv` or `bun` artifacts for this project.

## Important Notes

- **Broadcasts require approval**: real `broadcast send` and `broadcast resume` require `--confirm`; `--dry-run` does not send
- **Broadcasts are resumable**: if a send is interrupted, `broadcast send --confirm` or `broadcast resume --confirm` skips already-sent recipients
- **Large sends are chunked**: sends use 100-recipient batch chunks with a write-ahead attempt log. For a 1,000-recipient test, target a separate 1,000-member list/segment and send that broadcast with `--confirm`.
- **Atomic broadcast lock**: prevents double-send races; use `--force-unlock` only with `--confirm` when previous process is confirmed dead
- **UTM auto-injection**: outbound `<a>` tags get utm_source/medium/campaign automatically unless the anchor has `data-utm="off"`
- **Deliverability footer behavior**: sends include `List-Unsubscribe` and `List-Unsubscribe-Post` headers; generated unsubscribe body links opt out of UTM rewriting
- **Template quality warnings**: `template lint` warns on unstyled text links and semantic layout tags that are fragile in email clients
- **Plain-text URLs**: the text MIME alternative preserves anchor destinations as `Label (URL)`, including CTA and unsubscribe links
- **Inbox placement is not guaranteed**: `health` verifies the Resend sender domain, but DNS policy, sender reputation, content, recipient engagement, and complaint rate are outside local SQLite state
- **Unsubscribe backend still matters**: the configured `[unsubscribe].public_url` should resolve to a real endpoint that can honor one-click unsubscribe POSTs
- **Stripe link injection**: buy.stripe.com / checkout.stripe.com URLs get client_reference_id auto-added
- **GDPR erasure**: `contact erase <email> --confirm` creates suppression tombstone then deletes
- **Content snapshots**: broadcast HTML is snapshotted at send time for compliance audit trail
- **Env vars**: `MLC_UNSUBSCRIBE_SECRET` (required for send, min 16 bytes), `MLC_EMAIL_CLI_TIMEOUT_SEC` (default 120)

## Exit Codes

| Code | Meaning | Action |
|------|---------|--------|
| 0 | Success | Continue |
| 1 | Transient (IO, network, lock held, timeout) | Retry |
| 2 | Config error (missing email-cli, schema mismatch) | Fix setup |
| 3 | Bad input | Fix arguments |
| 4 | Rate limited | Wait and retry |
