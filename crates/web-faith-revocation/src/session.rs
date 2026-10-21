//! Not resuming sessions saved before the revocation data last changed.
//!
//! rustls skips certificate verification on resumption, and the chain a session was established
//! with is not reachable from a session store, so a store cannot re-check it. Instead each saved
//! session is tagged with the data's epoch as of the server's last verification, and one tagged
//! with an earlier epoch is dropped rather than resumed, making the connection do a full handshake
//! that the verifier sees.
//!
//! The tag is the epoch the verifier recorded, not the one current when the ticket arrives: a
//! ticket can reach a long-lived connection well after the data it was checked against has moved
//! on. A connection verified before an update and one verified after it, to one server, share a
//! record, so a late ticket from the older one can still take the newer epoch; the window is
//! those two overlapping.

// spec:REVOC#where-the-check-sits

use std::{
	collections::HashMap,
	sync::{Arc, Mutex},
};

use rustls::{
	NamedGroup,
	client::{
		ClientSessionMemoryCache, ClientSessionStore, Tls12ClientSessionValue,
		Tls13ClientSessionValue,
	},
};
use rustls_pki_types::ServerName;

use crate::store::Store;

/// rustls's own default capacity.
const CAPACITY: usize = 256;

/// A session store that forgets every session saved before the epoch moved.
#[derive(Debug)]
pub struct EpochSessionStore {
	inner: ClientSessionMemoryCache,
	store: Arc<Store>,
	/// The epoch each server's saved sessions were verified under.
	saved_under: Mutex<HashMap<ServerName<'static>, u64>>,
}

impl EpochSessionStore {
	pub(crate) fn new(store: Arc<Store>) -> Self {
		Self {
			inner: ClientSessionMemoryCache::new(CAPACITY),
			store,
			saved_under: Mutex::new(HashMap::new()),
		}
	}

	/// Whether `server_name`'s saved sessions predate the current epoch, in which case they are
	/// purged. With `saving`, the server's sessions are recorded as verified under the epoch its
	/// certificate was last verified at, which may already be behind the current one.
	fn stale(&self, server_name: &ServerName<'static>, saving: bool) -> bool {
		let current = self.store.current_epoch();
		let mut saved_under = self
			.saved_under
			.lock()
			.expect("the epoch map is only held for a lookup or insert");
		let stale = saved_under
			.get(server_name)
			.is_some_and(|&epoch| epoch < current);
		if stale {
			self.purge(server_name);
			saved_under.remove(server_name);
		}
		if saving {
			// The inner cache bounds itself by evicting servers this map cannot see go, so the map
			// is bounded separately. A server dropped from it has its sessions dropped too, as an
			// untagged session could not be told stale later.
			if saved_under.len() >= CAPACITY * 4
				&& !saved_under.contains_key(server_name)
				&& let Some(victim) = saved_under.keys().next().cloned()
			{
				self.purge(&victim);
				saved_under.remove(&victim);
			}
			saved_under.insert(server_name.clone(), self.store.verified_epoch(server_name));
		}
		stale
	}

	fn purge(&self, server_name: &ServerName<'static>) {
		self.inner.remove_tls12_session(server_name);
		while self.inner.take_tls13_ticket(server_name).is_some() {}
	}
}

impl ClientSessionStore for EpochSessionStore {
	fn set_kx_hint(&self, server_name: ServerName<'static>, group: NamedGroup) {
		self.inner.set_kx_hint(server_name, group);
	}

	fn kx_hint(&self, server_name: &ServerName<'_>) -> Option<NamedGroup> {
		self.inner.kx_hint(server_name)
	}

	fn set_tls12_session(&self, server_name: ServerName<'static>, value: Tls12ClientSessionValue) {
		self.stale(&server_name, true);
		self.inner.set_tls12_session(server_name, value);
	}

	fn tls12_session(&self, server_name: &ServerName<'_>) -> Option<Tls12ClientSessionValue> {
		if self.stale(&server_name.to_owned(), false) {
			return None;
		}
		self.inner.tls12_session(server_name)
	}

	fn remove_tls12_session(&self, server_name: &ServerName<'static>) {
		self.inner.remove_tls12_session(server_name);
	}

	fn insert_tls13_ticket(
		&self,
		server_name: ServerName<'static>,
		value: Tls13ClientSessionValue,
	) {
		self.stale(&server_name, true);
		self.inner.insert_tls13_ticket(server_name, value);
	}

	fn take_tls13_ticket(
		&self,
		server_name: &ServerName<'static>,
	) -> Option<Tls13ClientSessionValue> {
		if self.stale(server_name, false) {
			return None;
		}
		self.inner.take_tls13_ticket(server_name)
	}
}
