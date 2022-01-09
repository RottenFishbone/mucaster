#[derive(Debug)]
pub enum CastError {
    RustCastError(rust_cast::errors::Error),
    IoError(std::io::Error),
    HyperError(warp::hyper::Error),
    MDNSError(mdns::Error),
    ServerError,
    CasterError(&'static str),
}
impl From<rust_cast::errors::Error> for CastError {
    fn from(err: rust_cast::errors::Error) -> Self {
        CastError::RustCastError(err)
    }
}
impl From<std::io::Error> for CastError {
    fn from(err: std::io::Error) -> Self {
        CastError::IoError(err)
    }
}
impl From<warp::hyper::Error> for CastError {
    fn from(err: warp::hyper::Error) -> Self {
        CastError::HyperError(err)
    }
}
impl From<mdns::Error> for CastError {
    fn from(err: mdns::Error) -> Self {
        CastError::MDNSError(err)
   }
}
