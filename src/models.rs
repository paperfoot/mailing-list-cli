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
