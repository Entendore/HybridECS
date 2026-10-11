use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum FetchEntityError {
    NoSuchEntity,
    MissingComponent,
}

impl fmt::Display for FetchEntityError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FetchEntityError::NoSuchEntity => write!(f, "entity does not exist"),
            FetchEntityError::MissingComponent => write!(f, "entity is missing requested component"),
            _ => write!(f, "unknown entity error"),
        }
    }
}

impl std::error::Error for FetchEntityError {}
