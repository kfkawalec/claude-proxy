use bytes::Bytes;
use http_body_util::combinators::UnsyncBoxBody;

pub type ProxyBody = UnsyncBoxBody<Bytes, std::io::Error>;

pub mod backends;
pub mod error;
pub mod handler;
pub mod logging;
pub mod server;
pub mod stream;
