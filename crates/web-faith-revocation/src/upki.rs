//! The system's CRLite data: the cache that [upki] keeps current for every program on the host.
//!
//! upki fetches Mozilla's CRLite filters into a directory of its own on a timer, from a mirror its
//! administrator chose. Faith reads that directory and never writes to it, so a host with upki
//! has one copy of the filters rather than one per program, and a container that mounts the host's
//! cache needs no download of its own.
//!
//! [`locate`] finds the directory the way upki does, and [`read`] turns it into the same
//! [`Filters`](crate::crlite::Filters) the Remote Settings provider supplies. What upki writes is
//! described by [C2SP's upki-revocation](https://c2sp.org/upki-revocation) for `index.bin`; the
//! `manifest.json` beside it is upki's own.
//!
//! [upki]: https://github.com/rustls/upki

// spec:REVOC#the-systems-crlite-data

mod index;
mod locate;
mod read;

pub use locate::UpkiLocation;
pub(crate) use locate::Locator;
pub(crate) use read::read;
