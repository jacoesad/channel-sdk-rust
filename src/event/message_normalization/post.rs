use serde_json::Value;

pub(super) fn select_post_document(value: &Value) -> Option<&Value> {
    if has_legacy_post_content(value) {
        return Some(value);
    }

    for locale in ["zh_cn", "en_us", "ja_jp"] {
        if let Some(document) = value.get(locale).filter(|value| is_post_document(value)) {
            return Some(document);
        }
    }

    if has_content_v2(value) {
        return Some(value);
    }

    value
        .as_object()?
        .values()
        .find(|value| is_post_document(value))
}

fn is_post_document(value: &Value) -> bool {
    has_legacy_post_content(value) || has_content_v2(value)
}

fn has_legacy_post_content(value: &Value) -> bool {
    value.get("title").and_then(Value::as_str).is_some()
        || value.get("content").and_then(Value::as_array).is_some()
}

fn has_content_v2(value: &Value) -> bool {
    value.get("content_v2").and_then(Value::as_array).is_some()
}

pub(super) fn post_document_text(document: &Value) -> String {
    let mut lines = Vec::new();

    if let Some(title) = document.get("title").and_then(Value::as_str) {
        if !title.is_empty() {
            lines.push(title.to_owned());
        }
    }

    if let Some(content) = post_text_content(document) {
        lines.extend(
            content
                .iter()
                .filter_map(post_line_text)
                .filter(|line| !line.is_empty()),
        );
    }

    lines.join("\n")
}

pub(super) fn post_text_content(document: &Value) -> Option<&[Value]> {
    let content = document.get("content").and_then(Value::as_array);
    if let Some(lines) = content {
        if !lines.is_empty() {
            return Some(lines.as_slice());
        }
    }

    let content_v2 = document.get("content_v2").and_then(Value::as_array);
    if let Some(lines) = content_v2 {
        if !lines.is_empty() {
            return Some(lines.as_slice());
        }
    }

    content.map(Vec::as_slice)
}

fn post_line_text(line: &Value) -> Option<String> {
    let elements = line.as_array()?;
    let mut text = String::new();

    for element in elements {
        text.push_str(&post_element_text(element));
    }

    Some(text)
}

fn post_element_text(element: &Value) -> String {
    match element.get("tag").and_then(Value::as_str) {
        Some("at") => post_at_text(element),
        Some("text" | "a") => element_text(element),
        _ => element_text(element),
    }
}

fn element_text(element: &Value) -> String {
    element
        .get("text")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned()
}

fn post_at_text(element: &Value) -> String {
    let name = element
        .get("user_name")
        .or_else(|| element.get("text"))
        .and_then(Value::as_str)
        .unwrap_or_default();

    if name.is_empty() || name.starts_with('@') {
        name.to_owned()
    } else {
        format!("@{name}")
    }
}
