//! The facts about a certificate the checks need, read straight from its DER.
//!
//! OneCRL matches on the exact bytes of a certificate's issuer and subject names, so they are
//! taken as they appear in the certificate rather than decoded and re-encoded, which could
//! normalise them into bytes no entry names.

use sha2::{Digest, Sha256};

/// A certificate's signed timestamp from one Certificate Transparency log.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Sct {
	pub(crate) log_id: [u8; 32],
	/// Milliseconds since the Unix epoch.
	pub(crate) timestamp: u64,
}

/// What the revocation checks read from one certificate.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct CertInfo {
	/// The issuer `Name`, tag and length included.
	pub(crate) issuer: Vec<u8>,
	/// The subject `Name`, tag and length included.
	pub(crate) subject: Vec<u8>,
	/// The serial number's content octets, a leading zero included where the encoding has one.
	pub(crate) serial: Vec<u8>,
	/// SHA-256 of the whole `SubjectPublicKeyInfo`.
	pub(crate) spki_hash: [u8; 32],
	/// The subject key identifier, if the certificate carries one.
	pub(crate) ski: Option<Vec<u8>>,
	/// The authority key identifier's key identifier, if the certificate carries one.
	pub(crate) aki: Option<Vec<u8>>,
	/// The embedded SCTs. Empty when the certificate has none, or the list does not parse.
	pub(crate) scts: Vec<Sct>,
}

const OID_SKI: &[u8] = &[0x55, 0x1d, 0x0e];
const OID_AKI: &[u8] = &[0x55, 0x1d, 0x23];
const OID_EKU: &[u8] = &[0x55, 0x1d, 0x25];
const OID_SCT_LIST: &[u8] = &[0x2b, 0x06, 0x01, 0x04, 0x01, 0xd6, 0x79, 0x02, 0x04, 0x02];

const SEQUENCE: u8 = 0x30;
const INTEGER: u8 = 0x02;
const OCTET_STRING: u8 = 0x04;
const OID: u8 = 0x06;
const BOOLEAN: u8 = 0x01;
const VERSION: u8 = 0xa0;
const EXTENSIONS: u8 = 0xa3;
const AKI_KEY_ID: u8 = 0x80;

impl CertInfo {
	/// Read a certificate. `None` when it is not a well-formed X.509 certificate as far as these
	/// fields go; the platform verifier has accepted it, so that is not expected in practice.
	pub(crate) fn parse(der: &[u8]) -> Option<Self> {
		let (cert, _) = Der::new(der).expect(SEQUENCE)?;
		let (tbs, _) = Der::new(cert).expect(SEQUENCE)?;

		let mut tbs = Der::new(tbs);
		tbs.skip_if(VERSION)?;
		let (serial, _) = tbs.expect(INTEGER)?;
		let _signature = tbs.expect(SEQUENCE)?;
		let (_, issuer) = tbs.expect(SEQUENCE)?;
		let _validity = tbs.expect(SEQUENCE)?;
		let (_, subject) = tbs.expect(SEQUENCE)?;
		let (_, spki) = tbs.expect(SEQUENCE)?;

		let mut info = Self {
			issuer: issuer.to_vec(),
			subject: subject.to_vec(),
			serial: serial.to_vec(),
			spki_hash: Sha256::digest(spki).into(),
			ski: None,
			aki: None,
			scts: Vec::new(),
		};

		// The unique identifiers, where present, sit between the key and the extensions.
		while let Some((tag, value, _)) = tbs.next() {
			if tag == EXTENSIONS {
				info.read_extensions(value)?;
			}
		}

		Some(info)
	}

	fn read_extensions(&mut self, value: &[u8]) -> Option<()> {
		let (list, _) = Der::new(value).expect(SEQUENCE)?;
		let mut list = Der::new(list);
		while let Some((tag, extension, _)) = list.next() {
			if tag != SEQUENCE {
				return None;
			}
			let mut extension = Der::new(extension);
			let (oid, _) = extension.expect(OID)?;
			extension.skip_if(BOOLEAN)?;
			let (value, _) = extension.expect(OCTET_STRING)?;

			match oid {
				OID_SKI => {
					let (ski, _) = Der::new(value).expect(OCTET_STRING)?;
					self.ski = Some(ski.to_vec());
				}
				OID_AKI => {
					let (aki, _) = Der::new(value).expect(SEQUENCE)?;
					let mut aki = Der::new(aki);
					while let Some((tag, value, _)) = aki.next() {
						if tag == AKI_KEY_ID {
							self.aki = Some(value.to_vec());
						}
					}
				}
				OID_SCT_LIST => {
					self.scts = Der::new(value)
						.expect(OCTET_STRING)
						.and_then(|(list, _)| parse_sct_list(list))
						.unwrap_or_default();
				}
				_ => {}
			}
		}
		Some(())
	}

	/// Whether `issuer` could have issued this certificate: its subject is this certificate's
	/// issuer, and its key identifier matches this certificate's authority key identifier where
	/// both carry one.
	pub(crate) fn could_be_issued_by(&self, issuer: &CertInfo) -> bool {
		if self.issuer != issuer.subject {
			return false;
		}
		match (&self.aki, &issuer.ski) {
			(Some(aki), Some(ski)) => aki == ski,
			_ => true,
		}
	}
}

/// Whether a certificate's extended key usage extension lists `purpose`.
pub(crate) fn has_extended_key_usage(der: &[u8], purpose: &[u8]) -> bool {
	let found = (|| {
		let (cert, _) = Der::new(der).expect(SEQUENCE)?;
		let (tbs, _) = Der::new(cert).expect(SEQUENCE)?;
		let mut tbs = Der::new(tbs);
		while let Some((tag, value, _)) = tbs.next() {
			if tag != EXTENSIONS {
				continue;
			}
			let (list, _) = Der::new(value).expect(SEQUENCE)?;
			let mut list = Der::new(list);
			while let Some((_, extension, _)) = list.next() {
				let mut extension = Der::new(extension);
				let (oid, _) = extension.expect(OID)?;
				extension.skip_if(BOOLEAN)?;
				let (value, _) = extension.expect(OCTET_STRING)?;
				if oid == OID_EKU {
					let (purposes, _) = Der::new(value).expect(SEQUENCE)?;
					let mut purposes = Der::new(purposes);
					while let Some((tag, oid, _)) = purposes.next() {
						if tag == OID && oid == purpose {
							return Some(true);
						}
					}
				}
			}
		}
		Some(false)
	})();
	found.unwrap_or(false)
}

/// The TLS-encoded `SignedCertificateTimestampList` of RFC 6962, reduced to each SCT's log and
/// timestamp.
fn parse_sct_list(list: &[u8]) -> Option<Vec<Sct>> {
	let mut reader = Tls(list);
	let mut list = Tls(reader.vec16()?);
	if !reader.0.is_empty() {
		return None;
	}

	let mut scts = Vec::new();
	while !list.0.is_empty() {
		let mut sct = Tls(list.vec16()?);
		// Version 1 is the only one defined; a later one may lay its fields out differently.
		if sct.u8()? != 0 {
			continue;
		}
		let log_id = sct.bytes(32)?.try_into().ok()?;
		let timestamp = u64::from_be_bytes(sct.bytes(8)?.try_into().ok()?);
		scts.push(Sct { log_id, timestamp });
	}
	Some(scts)
}

/// A reader over TLS presentation-language encoding.
struct Tls<'a>(&'a [u8]);

impl<'a> Tls<'a> {
	fn bytes(&mut self, len: usize) -> Option<&'a [u8]> {
		if self.0.len() < len {
			return None;
		}
		let (head, rest) = self.0.split_at(len);
		self.0 = rest;
		Some(head)
	}

	fn u8(&mut self) -> Option<u8> {
		self.bytes(1).map(|b| b[0])
	}

	fn vec16(&mut self) -> Option<&'a [u8]> {
		let len = u16::from_be_bytes(self.bytes(2)?.try_into().ok()?);
		self.bytes(len.into())
	}
}

/// A reader over a sequence of DER elements, handing out each one's tag, its content, and the
/// whole element.
pub(crate) struct Der<'a>(&'a [u8]);

impl<'a> Der<'a> {
	pub(crate) fn new(input: &'a [u8]) -> Self {
		Self(input)
	}

	/// The next element as `(tag, content, whole element)`. Only single-byte tags and lengths of
	/// up to four octets are read, which is all a certificate's fields use.
	pub(crate) fn next(&mut self) -> Option<(u8, &'a [u8], &'a [u8])> {
		let input = self.0;
		let (&tag, rest) = input.split_first()?;
		if tag & 0x1f == 0x1f {
			return None;
		}
		let (&first, rest) = rest.split_first()?;
		let (len, rest) = if first < 0x80 {
			(usize::from(first), rest)
		} else {
			let octets = usize::from(first & 0x7f);
			if octets == 0 || octets > 4 || rest.len() < octets {
				return None;
			}
			let (len, rest) = rest.split_at(octets);
			let len = len
				.iter()
				.fold(0usize, |acc, &b| (acc << 8) | usize::from(b));
			(len, rest)
		};
		if rest.len() < len {
			return None;
		}
		let (content, rest) = rest.split_at(len);
		let whole = &input[..input.len() - rest.len()];
		self.0 = rest;
		Some((tag, content, whole))
	}

	/// The next element, which must carry `tag`, as `(content, whole element)`.
	pub(crate) fn expect(&mut self, tag: u8) -> Option<(&'a [u8], &'a [u8])> {
		match self.next()? {
			(found, content, whole) if found == tag => Some((content, whole)),
			_ => None,
		}
	}

	/// Skip the next element if it carries `tag`. `None` only when the input is malformed.
	fn skip_if(&mut self, tag: u8) -> Option<()> {
		let mut peek = Der(self.0);
		match peek.next() {
			Some((found, ..)) if found == tag => {
				self.0 = peek.0;
				Some(())
			}
			Some(_) => Some(()),
			None if self.0.is_empty() => Some(()),
			None => None,
		}
	}
}

#[cfg(test)]
mod tests;
