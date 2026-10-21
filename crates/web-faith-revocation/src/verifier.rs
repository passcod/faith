//! The certificate verifier: another verifier first, then the revocation checks.

// spec:REVOC#where-the-check-sits

use std::sync::Arc;

use rustls::{
	CertificateError, DigitallySignedStruct, DistinguishedName, OtherError, SignatureScheme,
	client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier},
};
use rustls_pki_types::{CertificateDer, ServerName, UnixTime};

use crate::{Refusals, Sources, error::Revoked, links::RootIndex, store::Store};

/// A [`ServerCertVerifier`] that runs the wrapped verifier, then refuses a chain it accepted when a
/// revocation source lists a certificate on it.
///
/// Built with [`Revocation::verifier`](crate::Revocation::verifier). Everything other than the
/// chain check itself, such as handshake signatures and the schemes offered, is the wrapped
/// verifier's.
#[derive(Debug)]
pub struct RevocationVerifier {
	pub(crate) inner: Arc<dyn ServerCertVerifier>,
	pub(crate) store: Arc<Store>,
	pub(crate) sources: Sources,
	pub(crate) extra_roots: Option<Arc<RootIndex>>,
	pub(crate) refusals: Arc<Refusals>,
}

impl ServerCertVerifier for RevocationVerifier {
	fn verify_server_cert(
		&self,
		end_entity: &CertificateDer<'_>,
		intermediates: &[CertificateDer<'_>],
		server_name: &ServerName<'_>,
		ocsp_response: &[u8],
		now: UnixTime,
	) -> Result<ServerCertVerified, rustls::Error> {
		// Read first: an update landing mid-check leaves this connection on the older epoch.
		let epoch = self.store.current_epoch();
		let verified = self.inner.verify_server_cert(
			end_entity,
			intermediates,
			server_name,
			ocsp_response,
			now,
		)?;

		match self.store.refusing_source(
			end_entity,
			intermediates,
			self.sources,
			self.extra_roots.as_deref(),
		) {
			None => {
				self.store.record_verified(server_name, epoch);
				Ok(verified)
			}
			Some(source) => {
				self.refusals.add(source);
				Err(rustls::Error::InvalidCertificate(CertificateError::Other(
					OtherError(Arc::new(Revoked { source })),
				)))
			}
		}
	}

	fn verify_tls12_signature(
		&self,
		message: &[u8],
		cert: &CertificateDer<'_>,
		dss: &DigitallySignedStruct,
	) -> Result<HandshakeSignatureValid, rustls::Error> {
		self.inner.verify_tls12_signature(message, cert, dss)
	}

	fn verify_tls13_signature(
		&self,
		message: &[u8],
		cert: &CertificateDer<'_>,
		dss: &DigitallySignedStruct,
	) -> Result<HandshakeSignatureValid, rustls::Error> {
		self.inner.verify_tls13_signature(message, cert, dss)
	}

	fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
		self.inner.supported_verify_schemes()
	}

	fn requires_raw_public_keys(&self) -> bool {
		self.inner.requires_raw_public_keys()
	}

	fn root_hint_subjects(&self) -> Option<&[DistinguishedName]> {
		self.inner.root_hint_subjects()
	}
}
