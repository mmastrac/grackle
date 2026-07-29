//! One heading in a rendered document.

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Heading {
    pub level: u8,
    /// Anchor id on the element.
    pub id: String,
    /// Visible text.
    pub text: String,
}
