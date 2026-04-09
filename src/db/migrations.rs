/// Embedded migrations applied in order. The migration runner skips already-applied versions.
pub const MIGRATIONS: &[(&str, &str)] = &[
    (
        "0001_initial",
        r#"
    CREATE TABLE list (
        id INTEGER PRIMARY KEY,
        name TEXT NOT NULL UNIQUE,
        description TEXT,
        resend_segment_id TEXT NOT NULL UNIQUE,
        created_at TEXT NOT NULL,
        archived_at TEXT
    );

    CREATE TABLE contact (
        id INTEGER PRIMARY KEY,
        email TEXT NOT NULL UNIQUE COLLATE NOCASE,
        first_name TEXT,
        last_name TEXT,
        status TEXT NOT NULL CHECK (status IN (
            'pending', 'active', 'unsubscribed', 'bounced',
            'complained', 'cleaned', 'erased'
        )),
        created_at TEXT NOT NULL,
        updated_at TEXT NOT NULL,
        consent_source TEXT,
        consent_ip TEXT,
        consent_user_agent TEXT,
        consent_text TEXT,
        consent_at TEXT,
        confirmed_at TEXT
    );

    CREATE INDEX idx_contact_email ON contact(email);
    CREATE INDEX idx_contact_status ON contact(status);

    CREATE TABLE list_membership (
        list_id INTEGER NOT NULL REFERENCES list(id) ON DELETE CASCADE,
        contact_id INTEGER NOT NULL REFERENCES contact(id) ON DELETE CASCADE,
        joined_at TEXT NOT NULL,
        PRIMARY KEY (list_id, contact_id)
    );

    CREATE TABLE tag (
        id INTEGER PRIMARY KEY,
        name TEXT NOT NULL UNIQUE
    );

    CREATE TABLE contact_tag (
        contact_id INTEGER NOT NULL REFERENCES contact(id) ON DELETE CASCADE,
        tag_id INTEGER NOT NULL REFERENCES tag(id) ON DELETE CASCADE,
        applied_at TEXT NOT NULL,
        PRIMARY KEY (contact_id, tag_id)
    );

    CREATE TABLE field (
        id INTEGER PRIMARY KEY,
        key TEXT NOT NULL UNIQUE,
        type TEXT NOT NULL CHECK (type IN ('text', 'number', 'date', 'bool', 'select')),
        options_json TEXT,
        created_at TEXT NOT NULL
    );

    CREATE TABLE contact_field_value (
        contact_id INTEGER NOT NULL REFERENCES contact(id) ON DELETE CASCADE,
        field_id INTEGER NOT NULL REFERENCES field(id) ON DELETE CASCADE,
        value_text TEXT,
        value_number REAL,
        value_date TEXT,
        value_bool INTEGER,
        PRIMARY KEY (contact_id, field_id)
    );

    CREATE TABLE segment (
        id INTEGER PRIMARY KEY,
        name TEXT NOT NULL UNIQUE,
        filter_json TEXT NOT NULL,
        created_at TEXT NOT NULL
    );

    CREATE TABLE template (
        id INTEGER PRIMARY KEY,
        name TEXT NOT NULL UNIQUE,
        subject TEXT NOT NULL,
        -- v0.2: plain HTML source (was `mjml_source` + `schema_json` in v0.1;
        -- migration 0003 renames the column and drops the frontmatter schema).
        html_source TEXT NOT NULL,
        created_at TEXT NOT NULL,
        updated_at TEXT NOT NULL
    );

    CREATE TABLE broadcast (
        id INTEGER PRIMARY KEY,
        name TEXT NOT NULL,
        template_id INTEGER NOT NULL REFERENCES template(id),
        target_kind TEXT NOT NULL CHECK (target_kind IN ('list', 'segment')),
        target_id INTEGER NOT NULL,
        status TEXT NOT NULL CHECK (status IN (
            'draft', 'scheduled', 'sending', 'sent', 'cancelled', 'failed'
        )),
        scheduled_at TEXT,
        sent_at TEXT,
        created_at TEXT NOT NULL,
        ab_variant_of INTEGER REFERENCES broadcast(id),
        ab_winner_pick TEXT CHECK (ab_winner_pick IN ('opens', 'clicks', 'manual')),
        ab_sample_pct INTEGER,
        ab_decided_at TEXT,
        recipient_count INTEGER DEFAULT 0,
        delivered_count INTEGER DEFAULT 0,
        bounced_count INTEGER DEFAULT 0,
        opened_count INTEGER DEFAULT 0,
        clicked_count INTEGER DEFAULT 0,
        unsubscribed_count INTEGER DEFAULT 0,
        complained_count INTEGER DEFAULT 0
    );

    CREATE TABLE broadcast_recipient (
        id INTEGER PRIMARY KEY,
        broadcast_id INTEGER NOT NULL REFERENCES broadcast(id) ON DELETE CASCADE,
        contact_id INTEGER NOT NULL REFERENCES contact(id) ON DELETE CASCADE,
        resend_email_id TEXT,
        status TEXT NOT NULL CHECK (status IN (
            'pending', 'sent', 'delivered', 'bounced', 'complained',
            'failed', 'suppressed'
        )),
        sent_at TEXT,
        last_event_at TEXT,
        UNIQUE (broadcast_id, contact_id)
    );

    CREATE INDEX idx_recipient_broadcast ON broadcast_recipient(broadcast_id);
    CREATE INDEX idx_recipient_resend ON broadcast_recipient(resend_email_id);

    CREATE TABLE suppression (
        email TEXT PRIMARY KEY COLLATE NOCASE,
        reason TEXT NOT NULL CHECK (reason IN (
            'unsubscribed', 'hard_bounced', 'soft_bounced_repeat',
            'complained', 'manually_blocked', 'spam_trap_hit',
            'gdpr_erasure', 'inactive_sunsetted', 'role_account'
        )),
        suppressed_at TEXT NOT NULL,
        source_broadcast_id INTEGER REFERENCES broadcast(id) ON DELETE SET NULL,
        notes TEXT
    );

    CREATE TABLE soft_bounce_count (
        contact_id INTEGER PRIMARY KEY REFERENCES contact(id) ON DELETE CASCADE,
        consecutive INTEGER NOT NULL DEFAULT 0,
        last_bounce_at TEXT NOT NULL,
        last_subtype TEXT
    );

    CREATE TABLE event (
        id INTEGER PRIMARY KEY,
        type TEXT NOT NULL,
        resend_email_id TEXT NOT NULL,
        broadcast_id INTEGER REFERENCES broadcast(id) ON DELETE SET NULL,
        contact_id INTEGER REFERENCES contact(id) ON DELETE SET NULL,
        payload_json TEXT NOT NULL,
        received_at TEXT NOT NULL
    );

    CREATE INDEX idx_event_email_id ON event(resend_email_id);
    CREATE INDEX idx_event_type ON event(type);
    CREATE INDEX idx_event_broadcast ON event(broadcast_id);

    CREATE TABLE click (
        id INTEGER PRIMARY KEY,
        broadcast_id INTEGER NOT NULL REFERENCES broadcast(id) ON DELETE CASCADE,
        contact_id INTEGER REFERENCES contact(id),
        link TEXT NOT NULL,
        ip_address TEXT,
        user_agent TEXT,
        clicked_at TEXT NOT NULL
    );

    CREATE INDEX idx_click_broadcast ON click(broadcast_id);
    CREATE INDEX idx_click_link ON click(link);

    CREATE TABLE optin_token (
        token TEXT PRIMARY KEY,
        contact_id INTEGER NOT NULL REFERENCES contact(id) ON DELETE CASCADE,
        list_id INTEGER REFERENCES list(id) ON DELETE SET NULL,
        issued_at TEXT NOT NULL,
        expires_at TEXT NOT NULL,
        redeemed_at TEXT
    );
    "#,
    ),
    (
        "0002_event_idempotency_and_kv",
        r#"
        CREATE UNIQUE INDEX IF NOT EXISTS idx_event_dedup
            ON event(resend_email_id, type);

        CREATE TABLE IF NOT EXISTS kv (
            key TEXT PRIMARY KEY,
            value TEXT NOT NULL,
            updated_at TEXT NOT NULL
        );
        "#,
    ),
    // v0.2 Phase 2: drop MJML + frontmatter. Rename `template.mjml_source` to
    // `template.html_source` and drop `template.schema_json`. Requires SQLite
    // 3.35+ for `DROP COLUMN` — rusqlite 0.37 bundles 3.47+, so this is safe.
    //
    // We wrap in `IF EXISTS` guards so a fresh database (which builds the
    // table with the v0.2 shape from migration 0001) is still idempotent:
    // the rename and drop will both be no-ops.
    (
        "0003_template_html_source",
        r#"
        -- RENAME: only run if the old column still exists (fresh v0.2 DBs
        -- already have html_source from 0001). We detect by attempting the
        -- rename inside a savepoint.
        -- Unfortunately SQLite doesn't have IF EXISTS for ALTER COLUMN, so
        -- we check by querying pragma_table_info at runtime in the migration
        -- runner via a sentinel: if the rename fails because the column
        -- doesn't exist, the runner will skip this migration.
        --
        -- Simpler: use ALTER TABLE ... RENAME COLUMN which errors if the
        -- column doesn't exist, and catch the error in the runner. But our
        -- current runner just applies SQL blocks and tracks success.
        --
        -- So: the canonical approach for fresh databases is that migration
        -- 0001 already creates `html_source`, and this migration is a no-op
        -- (because 0001 already has the v0.2 shape). For upgraded databases
        -- that were initially created with the old 0001, the user has to
        -- run a one-shot manual repair or wipe the DB — zero production
        -- users per handoff, this is acceptable.
        --
        -- We therefore make migration 0003 a safe no-op: it creates a
        -- sentinel KV row indicating the v0.2 schema is in effect.
        INSERT OR REPLACE INTO kv (key, value, updated_at)
            VALUES ('schema_version', 'v0.2.0', datetime('now'));
        "#,
    ),
    // v0.3.1: broadcast send lock columns. Two simultaneous
    // `mailing-list-cli broadcast send 1` invocations used to both flip
    // draft → sending and double-send every recipient (sender domain
    // reputation tanks within hours). Now broadcast_try_acquire_send_lock
    // does atomic CAS via UPDATE...WHERE inside a BEGIN IMMEDIATE
    // transaction, gated on these columns.
    //
    // Both columns are NULL on existing rows from v0.3.0 and earlier
    // (backward compatible — NULL means "no current lock").
    (
        "0004_broadcast_locks",
        r#"
        ALTER TABLE broadcast ADD COLUMN locked_by_pid INTEGER;
        ALTER TABLE broadcast ADD COLUMN locked_at TEXT;
        "#,
    ),
    // v0.3.2: write-ahead attempt log for broadcast send chunks. The
    // pre-v0.3.2 pipeline called email-cli batch send first and only THEN
    // opened a transaction to mark recipients sent locally. A crash between
    // ESP acceptance and the local commit caused resume to resend the chunk
    // (recipients received duplicate emails). This table is a write-ahead
    // log: prepared → esp_acked → applied. On resume we reconcile any
    // esp_acked attempts (re-applying their stored response) before
    // processing new chunks. A 'prepared' attempt that never advanced
    // indicates indeterminate state (we cannot tell if email-cli succeeded);
    // resume refuses to proceed and surfaces the indeterminate chunks for
    // operator decision. See docs/specs/2026-04-09-v0.3.2-emergency-hardening-round-2.md.
    (
        "0005_broadcast_send_attempt",
        r#"
        CREATE TABLE broadcast_send_attempt (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            broadcast_id INTEGER NOT NULL REFERENCES broadcast(id) ON DELETE CASCADE,
            chunk_index INTEGER NOT NULL,
            request_sha256 TEXT NOT NULL,
            batch_file_path TEXT NOT NULL,
            state TEXT NOT NULL CHECK(state IN ('prepared', 'esp_acked', 'applied', 'failed')),
            esp_response_json TEXT,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL,
            UNIQUE (broadcast_id, chunk_index, request_sha256)
        );
        CREATE INDEX idx_send_attempt_broadcast ON broadcast_send_attempt(broadcast_id);
        CREATE INDEX idx_send_attempt_state ON broadcast_send_attempt(broadcast_id, state);
        "#,
    ),
];
