use std::{fmt::Debug, pin::Pin};

use bytes::Bytes;
use futures::Stream;
use stream_shared::SharedStream;

pub(crate) type DynStream = dyn Stream<Item = std::result::Result<Bytes, String>> + Send + Sync;

pub(crate) enum Body {
	Inner(reqwest::Body),
	Consumed,
	Stream(SharedStream<Pin<Box<DynStream>>>),
}

impl Debug for Body {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		match self {
			Self::Inner(body) => write!(f, "{body:?}"),
			Self::Consumed => write!(f, "Consumed"),
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
