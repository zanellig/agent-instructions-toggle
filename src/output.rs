use std::path::Path;

pub fn path(path: &Path) -> String {
    text(&path.to_string_lossy())
}

pub fn text(value: &str) -> String {
    let mut rendered = String::new();
    for character in value.chars() {
        if character.is_control() {
            rendered.extend(character.escape_default());
        } else {
            match character {
                '<' => rendered.push_str("\\u{3c}"),
                '>' => rendered.push_str("\\u{3e}"),
                '&' => rendered.push_str("\\u{26}"),
                _ => rendered.push(character),
            }
        }
    }
    rendered
}
