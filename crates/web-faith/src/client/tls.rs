//! The rustls configuration Faith hands reqwest.
//!
//! reqwest builds its own rustls config from its `tls_*` builder calls, but offers no way to put a
//! verifier of our own in it. So Faith builds the whole config and passes it in preconfigured,
//! which makes everything reqwest would otherwise have set ours to set: the verifier, the client
//! identity, key logging, ALPN, and early data.

// spec:TLS

use std::{fmt, sync::Arc};

use rustls::{
	ClientConfig, DigitallySignedStruct, KeyLogFile, SignatureScheme,
	client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier},
	crypto::CryptoProvider,
};
use rustls_pki_types::{
	CertificateDer, PrivateKeyDer, ServerName, UnixTime,
	pem::{PemObject, SectionKind},
};

#[cfg(any(feature = "onecrl", feature = "crlite"))]
use rustls::client::Resumption;

#[cfg(any(feature = "onecrl", feature = "crlite"))]
use crate::revocation::RevocationRecipe;
use crate::{
	client::NodeEnvRecipe,
	error::{FaithError, FaithErrorKind},
};

/// A client certificate chain and its private key, presented for mTLS.
pub(crate) struct ClientIdentity {
	certs: Vec<CertificateDer<'static>>,
	key: PrivateKeyDer<'static>,
}

impl ClientIdentity {
	/// Parse a PEM bundle holding one private key (RSA, SEC1 EC, or PKCS#8) and at least one
	/// certificate. Where several keys are given, the last one is used.
	// spec:TLS#client-identity
	pub(crate) fn from_pem(pem: &[u8]) -> Result<Self, FaithError> {
		let invalid = |message: &str| FaithError::new(FaithErrorKind::PemParse, message);

		let mut certs = Vec::new();
		let mut key = None;
		let mut reader = pem;
		while let Some((kind, data)) = rustls_pki_types::pem::from_buf(&mut reader)
			.map_err(|_| invalid("invalid identity PEM file"))?
		{
			match kind {
				SectionKind::Certificate => certs.push(data.into()),
				SectionKind::PrivateKey => key = Some(PrivateKeyDer::Pkcs8(data.into())),
				SectionKind::RsaPrivateKey => key = Some(PrivateKeyDer::Pkcs1(data.into())),
				SectionKind::EcPrivateKey => key = Some(PrivateKeyDer::Sec1(data.into())),
				_ => return Err(invalid("unexpected section in identity PEM file")),
			}
		}

		match key {
			Some(key) if !certs.is_empty() => Ok(Self { certs, key }),
			_ => Err(invalid("private key or certificate not found")),
		}
	}
}

impl Clone for ClientIdentity {
	fn clone(&self) -> Self {
		Self {
			certs: self.certs.clone(),
			key: self.key.clone_key(),
		}
	}
}

impl fmt::Debug for ClientIdentity {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		f.debug_struct("ClientIdentity")
			.field("certs", &self.certs.len())
			.finish_non_exhaustive()
	}
}

/// Every certificate in a PEM bundle, in order. Sections other than certificates are skipped.
pub(crate) fn certs_from_pem(pem: &[u8]) -> Result<Vec<CertificateDer<'static>>, String> {
	CertificateDer::pem_slice_iter(pem)
		.collect::<Result<_, _>>()
		.map_err(|err| format!("invalid certificate encoding: {err}"))
}

/// The crypto provider the build selected, unless the embedding program installed a process
/// default of its own, which wins as it does for reqwest.
pub(crate) fn crypto_provider() -> Arc<CryptoProvider> {
	if let Some(provider) = CryptoProvider::get_default() {
		return Arc::clone(provider);
	}

	#[cfg(feature = "tls-aws-lc-rs")]
	return Arc::new(rustls::crypto::aws_lc_rs::default_provider());

	#[cfg(not(feature = "tls-aws-lc-rs"))]
	return Arc::new(rustls::crypto::ring::default_provider());
}

/// The agent's TLS settings, parsed at construction.
#[derive(Debug, Clone, Default)]
pub(crate) struct TlsRecipe {
	/// Only reachable over QUIC, so only applied when HTTP/3 is compiled in.
	#[cfg(feature = "http3")]
	pub(crate) early_data: bool,
	pub(crate) identity: Option<ClientIdentity>,
	pub(crate) extra_roots: Vec<CertificateDer<'static>>,
	/// The revocation sources the agent checks, if any.
	#[cfg(any(feature = "onecrl", feature = "crlite"))]
	pub(crate) revocation: Option<RevocationRecipe>,
}

impl TlsRecipe {
	/// Build the rustls config for the agent's HTTP connections.
	///
	/// reqwest clones it for QUIC and swaps the ALPN list for `h3`, so the one config serves both
	/// transports.
	pub(crate) fn config(&self, node_env: &NodeEnvRecipe) -> Result<ClientConfig, FaithError> {
		let config_error =
			|err: rustls::Error| FaithError::new(FaithErrorKind::Config, format!("{err:?}"));

		let provider = crypto_provider();
		let builder = ClientConfig::builder_with_provider(Arc::clone(&provider))
			.with_safe_default_protocol_versions()
			.map_err(config_error)?
			.dangerous();

		// spec:ENV#certificate-validation
		let builder = if node_env.accept_invalid_certs {
			builder.with_custom_certificate_verifier(Arc::new(NoVerifier))
		} else {
			let extra_roots: Vec<_> = self
				.extra_roots
				.iter()
				.chain(&node_env.extra_ca_certs)
				.cloned()
				.collect();
			let verifier = platform_verifier(&extra_roots, provider)?;
			builder.with_custom_certificate_verifier(self.with_revocation(verifier, &extra_roots))
		};

		let mut config = match &self.identity {
			Some(identity) => builder
				.with_client_auth_cert(identity.certs.clone(), identity.key.clone_key())
				.map_err(config_error)?,
			None => builder.with_no_client_auth(),
		};

		// Honours `SSLKEYLOGFILE`, and does nothing when it is unset.
		config.key_log = Arc::new(KeyLogFile::new());
		config.alpn_protocols = vec![b"h2".to_vec(), b"http/1.1".to_vec()];

		#[cfg(feature = "http3")]
		{
			config.enable_early_data = self.early_data;
		}

		self.resume_with_revocation(&mut config);
		Ok(config)
	}

	/// Build the rustls config for the resolver's encrypted DNS servers.
	///
	/// DNS keeps its own trust: the platform roots alone, whatever the agent's extra roots or
	/// `NODE_TLS_REJECT_UNAUTHORIZED` say, which is what hickory's default config trusts. The ALPN
	/// list stays empty so each transport fills in its own.
	///
	/// It gains the agent's revocation checks, which count their refusals with the agent's.
	// spec:DNS#server-order
	// spec:REVOC#where-the-check-sits
	#[cfg(feature = "dns")]
	pub(crate) fn dns_config(&self) -> Result<ClientConfig, FaithError> {
		let provider = crypto_provider();
		let verifier = platform_verifier(&[], Arc::clone(&provider))?;

		let mut config = ClientConfig::builder_with_provider(provider)
			.with_safe_default_protocol_versions()
			.map_err(|err| FaithError::new(FaithErrorKind::Config, format!("{err:?}")))?
			.dangerous()
			.with_custom_certificate_verifier(self.with_revocation(verifier, &[]))
			.with_no_client_auth();
		self.resume_with_revocation(&mut config);
		Ok(config)
	}

	/// Wrap `verifier` with the agent's revocation checks, if it has any. `extra_roots` are the
	/// roots it trusts beyond the platform's, so paths through them are linked too.
	// spec:REVOC#where-the-check-sits
	#[cfg_attr(
		not(any(feature = "onecrl", feature = "crlite")),
		allow(unused_variables)
	)]
	fn with_revocation(
		&self,
		verifier: Arc<dyn ServerCertVerifier>,
		extra_roots: &[CertificateDer<'static>],
	) -> Arc<dyn ServerCertVerifier> {
		#[cfg(any(feature = "onecrl", feature = "crlite"))]
		if let Some(revocation) = &self.revocation {
			return revocation.handle.verifier(
				verifier,
				revocation.sources,
				extra_roots,
				Arc::clone(&revocation.refusals),
			);
		}
		verifier
	}

	/// A resumed session skips the verifier, so with revocation checks on, sessions saved before
	/// the data last changed are not resumed.
	// spec:REVOC#where-the-check-sits
	#[cfg_attr(
		not(any(feature = "onecrl", feature = "crlite")),
		allow(unused_variables)
	)]
	fn resume_with_revocation(&self, config: &mut ClientConfig) {
		#[cfg(any(feature = "onecrl", feature = "crlite"))]
		if let Some(revocation) = &self.revocation {
			config.resumption = Resumption::store(revocation.handle.session_store());
		}
	}
}

/// The platform verifier, trusting `extra_roots` on top of the platform store.
// spec:TLS#trust-roots
fn platform_verifier(
	extra_roots: &[CertificateDer<'static>],
	provider: Arc<CryptoProvider>,
) -> Result<Arc<dyn ServerCertVerifier>, FaithError> {
	let config_error =
		|err: rustls::Error| FaithError::new(FaithErrorKind::Config, format!("{err:?}"));

	let verifier = if extra_roots.is_empty() {
		rustls_platform_verifier::Verifier::new(provider).map_err(config_error)?
	} else {
		// The Android verifier asks the system and has no way to add roots to that, so, as reqwest
		// did, extra roots there are a configuration error.
		#[cfg(not(target_os = "android"))]
		{
			rustls_platform_verifier::Verifier::new_with_extra_roots(extra_roots.to_vec(), provider)
				.map_err(config_error)?
		}
		#[cfg(target_os = "android")]
		{
			let _ = provider;
			return Err(FaithError::new(
				FaithErrorKind::Config,
				"rustls-platform-verifier could not load extra certs",
			));
		}
	};

	Ok(Arc::new(verifier))
}

/// Accepts every certificate, for `NODE_TLS_REJECT_UNAUTHORIZED=0`.
// spec:ENV#certificate-validation
#[derive(Debug)]
struct NoVerifier;

impl ServerCertVerifier for NoVerifier {
	fn verify_server_cert(
		&self,
		_end_entity: &CertificateDer<'_>,
		_intermediates: &[CertificateDer<'_>],
		_server_name: &ServerName<'_>,
		_ocsp_response: &[u8],
		_now: UnixTime,
	) -> Result<ServerCertVerified, rustls::Error> {
		Ok(ServerCertVerified::assertion())
	}

	fn verify_tls12_signature(
		&self,
		_message: &[u8],
		_cert: &CertificateDer<'_>,
		_dss: &DigitallySignedStruct,
	) -> Result<HandshakeSignatureValid, rustls::Error> {
		Ok(HandshakeSignatureValid::assertion())
	}

	fn verify_tls13_signature(
		&self,
		_message: &[u8],
		_cert: &CertificateDer<'_>,
		_dss: &DigitallySignedStruct,
	) -> Result<HandshakeSignatureValid, rustls::Error> {
		Ok(HandshakeSignatureValid::assertion())
	}

	fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
		vec![
			SignatureScheme::RSA_PKCS1_SHA1,
			SignatureScheme::ECDSA_SHA1_Legacy,
			SignatureScheme::RSA_PKCS1_SHA256,
			SignatureScheme::ECDSA_NISTP256_SHA256,
			SignatureScheme::RSA_PKCS1_SHA384,
			SignatureScheme::ECDSA_NISTP384_SHA384,
			SignatureScheme::RSA_PKCS1_SHA512,
			SignatureScheme::ECDSA_NISTP521_SHA512,
			SignatureScheme::RSA_PSS_SHA256,
			SignatureScheme::RSA_PSS_SHA384,
			SignatureScheme::RSA_PSS_SHA512,
			SignatureScheme::ED25519,
			SignatureScheme::ED448,
		]
	}
}
