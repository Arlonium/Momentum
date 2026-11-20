//! La ressource `mementum://spec/mmd` : la spécification du format MMD, que les descriptions des
//! outils ne recopient pas (§6.2). Son texte est celui de `spec/mmd/SPEC.md`, embarqué.

/// L'adresse de la ressource.
pub const MMD_URI: &str = "mementum://spec/mmd";
/// Son type.
pub const MMD_MIME: &str = "text/markdown";
/// Son texte (`spec/mmd/SPEC.md`).
pub const MMD: &str = include_str!("../../../spec/mmd/SPEC.md");

/// Une ressource servie.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Resource {
    pub uri: &'static str,
    pub name: &'static str,
    pub title: &'static str,
    pub description: &'static str,
    pub mime_type: &'static str,
    pub text: &'static str,
}

static RESOURCES: [Resource; 1] = [Resource {
    uri: MMD_URI,
    name: "spec-mmd",
    title: "Le format MMD",
    description: "La spécification du MMD (Mementum Markdown), le texte des pages que lisent et \
                  écrivent fetch, create_pages et update_page.",
    mime_type: MMD_MIME,
    text: MMD,
}];

/// Les ressources servies.
pub fn resources() -> &'static [Resource] {
    &RESOURCES
}

/// La ressource de cette adresse.
pub fn find(uri: &str) -> Option<&'static Resource> {
    RESOURCES.iter().find(|r| r.uri == uri)
}
