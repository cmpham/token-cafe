//! Lossless escaped-cell layout, shared by desktop and firmware.
use alloc::{format, string::String, vec::Vec};
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Line {
    pub offset: usize,
    pub text: String,
}
pub fn layout(text: &str, width: usize) -> Vec<Line> {
    assert!(width > 0);
    let mut result = Vec::new();
    let mut line = Line { offset: 0, text: String::new() };
    let mut cells = 0;
    for (offset, c) in text.char_indices() {
        if c == '\n' {
            result.push(line);
            line = Line { offset: offset + 1, text: String::new() };
            cells = 0;
            continue;
        }
        let escaped = match c {
            '\\' => String::from("\\\\"),
            '\r' => String::from("\\r"),
            '\t' => String::from("\\t"),
            c if c.is_ascii_control() => format!("\\x{:02x}", c as u32),
            c if !c.is_ascii() => format!("\\u{{{:x}}}", c as u32),
            c => String::from(c),
        };
        for ch in escaped.chars() {
            if cells == width {
                result.push(line);
                line = Line { offset, text: String::new() };
                cells = 0;
            }
            line.text.push(ch);
            cells += 1;
        }
    }
    result.push(line);
    result
}
pub fn index(lines: &[Line], anchor: usize) -> usize {
    {
        let offset = lines.iter().filter(|l| l.offset <= anchor).map(|l| l.offset).max().unwrap_or(0);
        lines.iter().position(|l| l.offset == offset).unwrap_or(0)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn preserves_controls_whitespace_and_trailing_line() {
        let l = layout("A  B\r\n\\\té\n", 54);
        assert_eq!(l.iter().map(|l| l.text.as_str()).collect::<Vec<_>>(), ["A  B\\r", "\\\\\\t\\u{e9}", ""]);
        assert_eq!(l[1].offset, 6);
    }
    #[test]
    fn hard_wrap_preserves_byte_boundaries() {
        let l = layout("123éx", 4);
        assert_eq!(l[0].text, "123\\");
        assert_eq!(l[1].offset, 3);
        assert_eq!(l[2].offset, 3);
        for line in &l {
            assert!("123éx".is_char_boundary(line.offset));
        }
    }
}
