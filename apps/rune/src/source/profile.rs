use serde::{Deserialize, Serialize};

/// Identifies whether a profile name came from the backend or the selected route.
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum ProfileIntent {
    #[default]
    Backend,
    NativeSelf,
}
impl ProfileIntent {
    pub fn is_backend(&self) -> bool {
        *self == Self::Backend
    }
}
