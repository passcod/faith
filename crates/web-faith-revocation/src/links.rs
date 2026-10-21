//! Linking a presented chain to every path the platform verifier could have built.
//!
//! The platform verifier says a trusted path exists without saying which, and servers send
//! certificates beyond that path, such as stale cross-signs, while leaving out the root. So the
//! checks link each certificate to every candidate that could have issued it, by name and key
//! identifier, among the presented certificates and the trust store's roots. Signatures are not
//! checked here: the platform verifier has already proved that a valid path exists, and this only
//! works out which certificates could be on it.

// spec:REVOC#linking-certificates

use std::collections::HashMap;

use rustls_pki_types::CertificateDer;

use crate::cert::CertInfo;

/// How many certificates a path may hold before the walk gives up on it, which passes it.
const MAX_DEPTH: usize = 8;

/// How many certificates one check may visit across every path, so a pathological chain cannot
/// make a handshake expensive. Running out passes the connection.
const MAX_VISITS: usize = 256;

/// Trust-store roots, looked up by subject.
#[derive(Debug, Default)]
pub(crate) struct RootIndex {
	by_subject: HashMap<Vec<u8>, Vec<CertInfo>>,
}

impl RootIndex {
	pub(crate) fn new<'a>(roots: impl IntoIterator<Item = &'a CertificateDer<'a>>) -> Self {
		let mut index = Self::default();
		for root in roots {
			if let Some(info) = CertInfo::parse(root) {
				index
					.by_subject
					.entry(info.subject.clone())
					.or_default()
					.push(info);
			}
		}
		index
	}

	/// The platform's own roots, as `rustls-native-certs` lists them. Roots that fail to load are
	/// left out, so a path through them ends before the root.
	pub(crate) fn platform() -> Self {
		let roots = rustls_native_certs::load_native_certs().certs;
		Self::new(roots.iter())
	}

	fn issuers_of<'a>(&'a self, cert: &'a CertInfo) -> impl Iterator<Item = &'a CertInfo> {
		self.by_subject
			.get(&cert.issuer)
			.into_iter()
			.flatten()
			.filter(|candidate| cert.could_be_issued_by(candidate))
	}
}

/// One handshake's certificates, with the roots they may lead to.
pub(crate) struct Links<'a> {
	pub(crate) leaf: CertInfo,
	presented: Vec<CertInfo>,
	roots: [Option<&'a RootIndex>; 2],
}

impl<'a> Links<'a> {
	/// `None` when the leaf does not parse, in which case nothing is checked.
	pub(crate) fn new(
		end_entity: &CertificateDer<'_>,
		intermediates: &[CertificateDer<'_>],
		platform_roots: Option<&'a RootIndex>,
		extra_roots: Option<&'a RootIndex>,
	) -> Option<Self> {
		Some(Self {
			leaf: CertInfo::parse(end_entity)?,
			presented: intermediates
				.iter()
				.filter_map(|cert| CertInfo::parse(cert))
				.collect(),
			roots: [platform_roots, extra_roots],
		})
	}

	/// Every candidate issuer of `cert`, from the presented certificates and the roots.
	pub(crate) fn issuers_of<'s>(&'s self, cert: &'s CertInfo) -> Vec<&'s CertInfo> {
		let presented = self
			.presented
			.iter()
			.filter(|candidate| *candidate != cert && cert.could_be_issued_by(candidate));
		let roots = self
			.roots
			.iter()
			.flatten()
			.flat_map(|index| index.issuers_of(cert));

		let mut issuers: Vec<&CertInfo> = Vec::new();
		for candidate in presented.chain(roots) {
			// A root the server also sent is one candidate, not two.
			if candidate != cert && !issuers.contains(&candidate) {
				issuers.push(candidate);
			}
		}
		issuers
	}

	/// Whether every path from the leaf contains a certificate `listed` matches.
	pub(crate) fn every_path_has(&self, listed: impl Fn(&CertInfo) -> bool) -> bool {
		let mut visits = 0;
		let mut path = vec![&self.leaf];
		self.blocked(&listed, &mut path, &mut visits)
	}

	fn blocked<'s>(
		&'s self,
		listed: &impl Fn(&CertInfo) -> bool,
		path: &mut Vec<&'s CertInfo>,
		visits: &mut usize,
	) -> bool {
		*visits += 1;
		let cert = *path.last().expect("a path always holds the leaf");
		if listed(cert) {
			return true;
		}
		if path.len() >= MAX_DEPTH || *visits > MAX_VISITS {
			return false;
		}

		let issuers: Vec<_> = self
			.issuers_of(cert)
			.into_iter()
			.filter(|issuer| !path.contains(issuer))
			.collect();
		// A path that ends here, at a root or at a certificate whose issuer was not presented and
		// is not held, contains nothing listed.
		if issuers.is_empty() {
			return false;
		}

		issuers.into_iter().all(|issuer| {
			path.push(issuer);
			let blocked = self.blocked(listed, path, visits);
			path.pop();
			blocked
		})
	}
}

#[cfg(test)]
mod tests;
