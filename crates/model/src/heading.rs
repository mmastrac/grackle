//! One heading of a rendered document (§6e).

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Heading {
    pub level: u8,
    /// The anchor target emitted on the element.
    pub id: String,
    /// The heading's visible text, entities folded back.
    pub text: String,
}
