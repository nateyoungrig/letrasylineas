//! Compatibility wrapper for callers of the original Anthropic client.
pub const DEFAULT_MODEL: &str = "claude-opus-5-5";

pub struct Claude(crate::Ai);
impl Claude {
    pub fn new(api_key: impl Into<String>) -> Self {
        Self(
            crate::Ai::new(
                crate::provider::Provider::Anthropic,
                DEFAULT_MODEL.into(),
                Some(api_key.into()),
            )
            .expect("valid default Anthropic configuration"),
        )
    }
}
impl std::ops::Deref for Claude {
    type Target = crate::Ai;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
