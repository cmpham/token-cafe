//! Provider-independent request data and literal text layout.
use alloc::{string::String, vec::Vec};
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Request {
    pub id: String,
    pub session: String,
    pub title: String,
    pub tool_name: String,
    pub text: String,
    /// Numbered single-choice options rendered verbatim in text (0 = approval).
    pub choices: u8,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Decision {
    Allow,
    Deny,
    Choice(u8),
}
/// Literal cells: do not silently hide Unicode, control characters or command case.
pub fn lines(text: &str, width: usize) -> Vec<String> {
    crate::text::layout(text, width).into_iter().map(|line| line.text).collect()
}

impl Decision {
    pub fn code(self) -> i32 {
        match self {
            Self::Allow => 1,
            Self::Deny => 2,
            Self::Choice(n) => 3 + n as i32,
        }
    }
    pub fn from_code(code: i32) -> Option<Self> {
        match code {
            1 => Some(Self::Allow),
            2 => Some(Self::Deny),
            3..=6 => Some(Self::Choice((code - 3) as u8)),
            _ => None,
        }
    }
}
