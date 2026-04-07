use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct List {
    pub id: i64,
    pub name: String,
    pub description: Option<String>,
    pub resend_audience_id: String,
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
