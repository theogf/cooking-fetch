use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct Recipe {
    pub id: Option<i32>,
    pub name: String,
    pub page_start: i32,
    pub page_end: i32,
    pub has_picture: bool,
}
