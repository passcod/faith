use std::{fmt::Debug, pin::Pin};

use bytes::Bytes;
use futures::Stream;
use stream_shared::SharedStream;

pub(crate) type DynStream = dyn Stream<Item = std::result::Result<Bytes, String>> + Send + Sync;

#[derive(Clone)]
pub(crate) enum Body {
    None,
    Stream(SharedStream<Pin<Box<DynStream>>>),
}

impl Debug for Body {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::None => write!(f, "None"),
            Self::Stream(stream) => {
                let field = f
                    .debug_struct("SharedStream")
                    .field("stats", &stream.stats())
                    .finish_non_exhaustive();
                f.debug_tuple("Stream").field(&field).finish()
            }
        }
    }
}
