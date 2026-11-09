//! Contenu en ligne : marques de texte et types en ligne connus de la P1.

/// Styles (marques) de texte reconnus.
pub const STYLES: &[&str] = &[
    "bold",
    "italic",
    "underline",
    "strike",
    "code",
    "textColor",
    "backgroundColor",
];

/// Types de contenu en ligne autres que le texte.
pub const INLINE_TYPES: &[&str] = &["link", "mention", "math", "hardBreak"];
