use crate::{Error, Result};

pub(crate) fn validate_path_identifier(value: &str, name: &str) -> Result<()> {
    if value.is_empty() {
        return Err(Error::Validation(format!("{name} must not be empty")));
    }
    if !value
        .chars()
        .all(|character| character.is_ascii_alphanumeric() || matches!(character, '_' | '-'))
    {
        return Err(Error::Validation(format!(
            "{name} must contain only ASCII letters, digits, underscores, or hyphens"
        )));
    }
    Ok(())
}
