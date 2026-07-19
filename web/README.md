# SharpClap Web

Vercel product and companion app for `mailing-list-cli`.

It owns the public product surface and the web endpoints that a local CLI cannot
safely provide:

- `GET /` for the SharpClap landing page and inquiry form.
- `POST /api/inquiries` for Neon-backed product and integration inquiries.
- `GET /u/:token` for visible unsubscribe footer links.
- `POST /u/:token` for RFC 8058 one-click unsubscribe requests.
- `GET /api/unsubscribes?after=<id>` for CLI sync, protected by `SYNC_API_KEY`.
- `GET /api/health` for runtime and database readiness.

Runtime storage is Neon Postgres. Blob is not used. Redis is not the source of
truth because unsubscribe and inquiry records are durable operational data.

Required Vercel env:

```bash
DATABASE_URL=...
MLC_UNSUBSCRIBE_SECRET=...
SYNC_API_KEY=...
NEXT_PUBLIC_SITE_URL=https://sharpclap.com
```

The Rust CLI must use the same `MLC_UNSUBSCRIBE_SECRET` and should set
`MLC_UNSUBSCRIBE_SYNC_KEY` to the Vercel `SYNC_API_KEY`.
