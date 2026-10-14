//! Provider-independent request data and literal text layout.
use alloc::{string::String, vec::Vec};
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Request {
    pub id: String,
    pub session: String,
    pub title: String,
    pub tool_name: String,
    pub text: String,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Decision {
    Allow,
    Deny,
}
/// Literal cells: do not silently hide Unicode, control characters or command case.
pub fn lines(text: &str, width: usize) -> Vec<String> {
    crate::text::layout(text, width).into_iter().map(|line| line.text).collect()
}
