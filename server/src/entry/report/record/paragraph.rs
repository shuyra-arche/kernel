use alloc::{string::String, vec::Vec};

pub struct Paragraph {
    pub title: Option<String>,
    pub items: Vec<Vec<Option<String>>>,
}
