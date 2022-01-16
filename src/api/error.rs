use crate::cast;

#[derive(Debug)]
pub enum ApiError {
    ApiError(String),
    CastError(cast::Error)
}

// ApiError from string
impl From<String> for ApiError {
    fn from(s: String) -> Self {
        Self::ApiError(s)
    }
}

// CastError
impl From<cast::Error> for ApiError {
    fn from(e: cast::Error) -> Self {
        Self::CastError(e)
    }
}
