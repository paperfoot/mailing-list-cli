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
#[allow(dead_code)] // Wired up in Task 8
pub struct Tag {
    pub id: i64,
    pub name: String,
    pub member_count: i64,
}
