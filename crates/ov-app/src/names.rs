//! The greeting's first name, from the Windows account name. Never leaves the
//! machine.

/// Pull a presentable first name out of a Windows account name.
///
/// Account names are typically `first.last`, `FIRST_LAST` or a bare first name,
/// and Windows imposes no casing convention -- `ADITYA` and `aditya` are both
/// real accounts. Splitting on the separators a domain or local account name
/// actually uses and title-casing the first token turns any of those into what
/// the Home screen's greeting wants to say.
pub fn first_name(username: &str) -> Option<String> {
    let token = username
        .split(['.', '_', ' '])
        .find(|t| !t.trim().is_empty())?
        .trim();
    let mut chars = token.chars();
    let first = chars.next()?;
    Some(
        first
            .to_uppercase()
            .chain(chars.flat_map(|c| c.to_lowercase()))
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::first_name;

    #[test]
    fn first_token_capitalised() {
        assert_eq!(first_name("aditya.shelke").as_deref(), Some("Aditya"));
        assert_eq!(first_name("ADITYA_SHELKE").as_deref(), Some("Aditya"));
        assert_eq!(first_name("maria jose").as_deref(), Some("Maria"));
        assert_eq!(first_name("  .bob").as_deref(), Some("Bob"));
        assert_eq!(first_name("émile").as_deref(), Some("Émile"));
        assert_eq!(first_name(""), None);
        assert_eq!(first_name(" ._ "), None);
    }
}
