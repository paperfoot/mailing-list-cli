use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct List {
    pub id: i64,
    pub name: String,
    pub description: Option<String>,
    pub resend_segment_id: String,
    pub created_at: String,
    pub member_count: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct Contact {
    pub id: i64,
    pub email: String,
    pub first_name: Option<String>,
    pub last_name: Option<String>,
    pub status: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct Tag {
    pub id: i64,
    pub name: String,
    pub member_count: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct Field {
    pub id: i64,
    pub key: String,
    pub r#type: String, // "text" | "number" | "date" | "bool" | "select"
    pub options: Option<Vec<String>>, // deserialized from options_json for select
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct Segment {
    pub id: i64,
    pub name: String,
    pub filter_json: String,
    pub created_at: String,
    pub member_count: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct Template {
    pub id: i64,
    pub name: String,
    pub subject: String,
    /// Plain HTML with `{{ var }}` merge tags. v0.2 dropped MJML + frontmatter
    /// schemas — this is literally the HTML body the agent wrote.
    pub html_source: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize)]
#[allow(dead_code)]
pub struct Broadcast {
    pub id: i64,
    pub name: String,
    pub template_id: i64,
    pub target_kind: String, // "list" | "segment"
    pub target_id: i64,
    pub status: String, // draft/scheduled/sending/sent/cancelled/failed
    pub scheduled_at: Option<String>,
    pub sent_at: Option<String>,
    pub created_at: String,
    pub recipient_count: i64,
    pub delivered_count: i64,
    pub bounced_count: i64,
    pub opened_count: i64,
    pub clicked_count: i64,
    pub unsubscribed_count: i64,
    pub complained_count: i64,
}

#[derive(Debug, Clone, Serialize)]
#[allow(dead_code)]
pub struct BroadcastRecipient {
    pub id: i64,
    pub broadcast_id: i64,
    pub contact_id: i64,
    pub resend_email_id: Option<String>,
    pub status: String, // pending/sent/delivered/bounced/complained/failed/suppressed
    pub sent_at: Option<String>,
    pub last_event_at: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ReportSummary {
    pub broadcast_id: i64,
    pub broadcast_name: String,
    pub recipient_count: i64,
    pub delivered_count: i64,
    pub bounced_count: i64,
    pub opened_count: i64,
    pub clicked_count: i64,
    pub unsubscribed_count: i64,
    pub complained_count: i64,
    pub suppressed_count: i64,
    pub ctr: f64,
    pub bounce_rate: f64,
    pub complaint_rate: f64,
    pub open_rate: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct LinkReport {
    pub link: String,
    pub clicks: i64,
    pub unique_clickers: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct DeliverabilityReport {
    pub window_days: i64,
    pub total_sent: i64,
    pub total_delivered: i64,
    pub total_bounced: i64,
    pub total_complained: i64,
    pub bounce_rate: f64,
    pub complaint_rate: f64,
    pub verified_domains: Vec<String>,
}
