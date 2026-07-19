import { neon } from "@neondatabase/serverless";
import { createHash } from "crypto";
import { requireDatabaseUrl, requireUnsubscribeSecret } from "./config";
import type { VerifiedUnsubscribeToken } from "./unsubscribe-token";

type Sql = ReturnType<typeof neon>;

export type UnsubscribeMethod = "GET" | "POST";

export type StoredUnsubscribeEvent = {
  id: number;
  token: string;
  contact_id: number;
  broadcast_id: number;
  issued_at: number;
  method: string;
  last_method: string;
  seen_count: number;
  user_agent: string | null;
  ip_hash: string | null;
  created_at: string;
  last_seen_at: string;
};

export type StoredInquiry = {
  id: number;
  name: string;
  reply_contact: string;
  organization: string | null;
  list_size: string | null;
  agent_stack: string | null;
  message: string;
  source: string;
  user_agent: string | null;
  created_at: string;
};

let sqlClient: Sql | null = null;
let schemaReady: Promise<void> | null = null;

export function getSql(): Sql {
  if (!sqlClient) {
    sqlClient = neon(requireDatabaseUrl());
  }
  return sqlClient;
}

export async function ensureSchema(): Promise<void> {
  if (!schemaReady) {
    const sql = getSql();
    schemaReady = sql`
      create table if not exists mlc_unsubscribe_event (
        id bigserial primary key,
        token text not null unique,
        contact_id bigint not null,
        broadcast_id bigint not null,
        issued_at bigint not null,
        method text not null check (method in ('GET', 'POST')),
        last_method text not null check (last_method in ('GET', 'POST')),
        seen_count integer not null default 1,
        user_agent text,
        ip_hash text,
        created_at timestamptz not null default now(),
        last_seen_at timestamptz not null default now()
      )
    `.then(async () => {
      await sql`
        create index if not exists idx_mlc_unsubscribe_event_contact
          on mlc_unsubscribe_event (contact_id)
      `;
      await sql`
        create index if not exists idx_mlc_unsubscribe_event_broadcast
          on mlc_unsubscribe_event (broadcast_id)
      `;
      await sql`
        create index if not exists idx_mlc_unsubscribe_event_created
          on mlc_unsubscribe_event (created_at)
      `;
      await sql`
        create table if not exists mlc_inquiry (
          id bigserial primary key,
          name text not null,
          reply_contact text not null,
          organization text,
          list_size text,
          agent_stack text,
          message text not null,
          source text not null default 'sharpclap_home',
          user_agent text,
          created_at timestamptz not null default now()
        )
      `;
      await sql`
        create index if not exists idx_mlc_inquiry_created
          on mlc_inquiry (created_at)
      `;
    });
  }

  return schemaReady;
}

export function hashIpAddress(ip: string | null): string | null {
  if (!ip) return null;
  return createHash("sha256")
    .update(ip)
    .update(":")
    .update(requireUnsubscribeSecret())
    .digest("hex");
}

export async function recordUnsubscribeEvent({
  verified,
  method,
  userAgent,
  ipHash,
}: {
  verified: VerifiedUnsubscribeToken;
  method: UnsubscribeMethod;
  userAgent: string | null;
  ipHash: string | null;
}): Promise<StoredUnsubscribeEvent> {
  await ensureSchema();
  const rows = (await getSql()`
    insert into mlc_unsubscribe_event (
      token,
      contact_id,
      broadcast_id,
      issued_at,
      method,
      last_method,
      user_agent,
      ip_hash
    )
    values (
      ${verified.token},
      ${verified.contactId},
      ${verified.broadcastId},
      ${verified.issuedAt},
      ${method},
      ${method},
      ${userAgent},
      ${ipHash}
    )
    on conflict (token) do update set
      seen_count = mlc_unsubscribe_event.seen_count + 1,
      last_method = excluded.last_method,
      last_seen_at = now(),
      user_agent = coalesce(excluded.user_agent, mlc_unsubscribe_event.user_agent),
      ip_hash = coalesce(excluded.ip_hash, mlc_unsubscribe_event.ip_hash)
    returning
      id,
      token,
      contact_id,
      broadcast_id,
      issued_at,
      method,
      last_method,
      seen_count,
      user_agent,
      ip_hash,
      created_at::text,
      last_seen_at::text
  `) as unknown as StoredUnsubscribeEvent[];

  return rows[0];
}

export async function listUnsubscribeEvents({
  after,
  limit,
}: {
  after: number;
  limit: number;
}): Promise<StoredUnsubscribeEvent[]> {
  await ensureSchema();
  return (await getSql()`
    select
      id,
      token,
      contact_id,
      broadcast_id,
      issued_at,
      method,
      last_method,
      seen_count,
      user_agent,
      ip_hash,
      created_at::text,
      last_seen_at::text
    from mlc_unsubscribe_event
    where id > ${after}
    order by id asc
    limit ${limit}
  `) as unknown as StoredUnsubscribeEvent[];
}

export async function recordInquiry({
  name,
  replyContact,
  organization,
  listSize,
  agentStack,
  message,
  source,
  userAgent,
}: {
  name: string;
  replyContact: string;
  organization: string | null;
  listSize: string | null;
  agentStack: string | null;
  message: string;
  source: string;
  userAgent: string | null;
}): Promise<StoredInquiry> {
  await ensureSchema();
  const rows = (await getSql()`
    insert into mlc_inquiry (
      name,
      reply_contact,
      organization,
      list_size,
      agent_stack,
      message,
      source,
      user_agent
    )
    values (
      ${name},
      ${replyContact},
      ${organization},
      ${listSize},
      ${agentStack},
      ${message},
      ${source},
      ${userAgent}
    )
    returning
      id,
      name,
      reply_contact,
      organization,
      list_size,
      agent_stack,
      message,
      source,
      user_agent,
      created_at::text
  `) as unknown as StoredInquiry[];

  return rows[0];
}

export async function pingDatabase(): Promise<void> {
  await ensureSchema();
  await getSql()`select 1`;
}
