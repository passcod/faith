//! A fake Remote Settings server, a content-signing chain for it, and a TLS chain to check, for
//! tests of this crate and of clients built on it. Not for production use.
//!
//! The server is plain HTTP on loopback with its own thread, so it serves whichever runtime the
//! poller is on.

#![allow(missing_docs, clippy::missing_panics_doc)]

use std::{
	collections::HashMap,
	io::{BufRead, BufReader, Write},
	net::{SocketAddr, TcpListener, TcpStream},
	sync::{Arc, Mutex},
	time::Duration,
};

use base64::{Engine, prelude::BASE64_STANDARD, prelude::BASE64_URL_SAFE_NO_PAD};
use rcgen::{
	BasicConstraints, CertificateParams, CertifiedIssuer, CustomExtension, DnType,
	ExtendedKeyUsagePurpose, IsCa, KeyPair, KeyUsagePurpose, SerialNumber, SigningKey,
};
use reqwest::Url;
use rustls::crypto::CryptoProvider;
use rustls_pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer, ServerName};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::{MOZILLA_SIGNER, RevocationConfig, cert::Der, signature::signed_message};

/// The crypto provider tests use.
pub fn provider() -> Arc<CryptoProvider> {
	Arc::new(rustls::crypto::aws_lc_rs::default_provider())
}

fn p384() -> KeyPair {
	KeyPair::generate_for(&rcgen::PKCS_ECDSA_P384_SHA384).expect("P-384 key generation")
}

fn ca_params(name: &str) -> CertificateParams {
	let mut params = CertificateParams::new(Vec::<String>::new()).expect("empty SANs");
	params.distinguished_name.push(DnType::CommonName, name);
	params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
	params.key_usages = vec![KeyUsagePurpose::KeyCertSign, KeyUsagePurpose::CrlSign];
	params.use_authority_key_identifier_extension = true;
	params
}

/// A content-signing chain shaped like Mozilla's: a root, an intermediate, and a code-signing
/// leaf naming the OneCRL signer.
pub struct SigningPki {
	root: CertificateDer<'static>,
	chain_pem: String,
	leaf_key: KeyPair,
}

impl SigningPki {
	pub fn new() -> Self {
		Self::with_signer(MOZILLA_SIGNER)
	}

	/// A chain whose leaf names `signer` instead.
	pub fn with_signer(signer: &str) -> Self {
		let root = CertifiedIssuer::self_signed(ca_params("Test content signing root"), p384())
			.expect("root");
		let intermediate = CertifiedIssuer::signed_by(
			ca_params("Test content signing intermediate"),
			p384(),
			&root,
		)
		.expect("intermediate");

		let leaf_key = p384();
		let mut params = CertificateParams::new(vec![signer.to_owned()]).expect("valid SAN");
		params.distinguished_name.push(DnType::CommonName, signer);
		params.extended_key_usages = vec![ExtendedKeyUsagePurpose::CodeSigning];
		params.key_usages = vec![KeyUsagePurpose::DigitalSignature];
		params.use_authority_key_identifier_extension = true;
		let leaf = params.signed_by(&leaf_key, &intermediate).expect("leaf");

		let chain_pem = [leaf.pem(), intermediate.pem(), root.pem()].concat();
		Self {
			root: root.der().clone(),
			chain_pem,
			leaf_key,
		}
	}

	pub fn root_hash(&self) -> [u8; 32] {
		Sha256::digest(&self.root).into()
	}

	pub fn chain_pem(&self) -> &str {
		&self.chain_pem
	}

	/// A content signature over `records` at `timestamp`, as Remote Settings serves it.
	pub fn sign(&self, records: &[Value], timestamp: u64) -> String {
		let der = self
			.leaf_key
			.sign(&signed_message(records, timestamp))
			.expect("signing");
		BASE64_URL_SAFE_NO_PAD.encode(der_to_raw(&der))
	}

	/// A config pointing at `server`, trusting this chain and caching in `cache_dir`.
	pub fn config(
		&self,
		server: &FakeRemoteSettings,
		cache_dir: Option<std::path::PathBuf>,
	) -> RevocationConfig {
		let mut config = RevocationConfig::mozilla(provider());
		config.base_url = server.base_url();
		config.root_hash = self.root_hash();
		config.cache_dir = cache_dir;
		config
	}
}

impl Default for SigningPki {
	fn default() -> Self {
		Self::new()
	}
}

/// A DER `ECDSA-Sig-Value` as the raw P-384 `r‖s` Remote Settings serves.
fn der_to_raw(der: &[u8]) -> Vec<u8> {
	let (seq, _) = Der::new(der).expect(0x30).expect("a DER signature");
	let mut seq = Der::new(seq);
	let mut raw = Vec::with_capacity(96);
	for _ in 0..2 {
		let (int, _) = seq.expect(0x02).expect("an INTEGER");
		let int = &int[int.len().saturating_sub(48)..];
		raw.extend(std::iter::repeat_n(0, 48 - int.len()));
		raw.extend_from_slice(int);
	}
	raw
}

/// The CT log the test leaves carry SCTs from.
pub const TEST_LOG_ID: [u8; 32] = [0x11; 32];

/// The SCT timestamp the test leaves carry, in milliseconds.
pub const TEST_SCT_TIMESTAMP: u64 = 1_700_000_000_000;

/// A server chain: a root, an intermediate, and a leaf for `localhost`.
pub struct TlsPki {
	pub root: CertificateDer<'static>,
	pub intermediate: CertificateDer<'static>,
	pub leaf: CertificateDer<'static>,
	pub leaf_key: PrivateKeyDer<'static>,
	/// The leaf's serial number, as CRLite takes it.
	pub leaf_serial: Vec<u8>,
	/// SHA-256 of the intermediate's public key, as CRLite takes it.
	pub intermediate_spki_hash: [u8; 32],
}

impl TlsPki {
	/// A chain whose leaf carries an SCT from [`TEST_LOG_ID`] when `with_sct` is set.
	pub fn new(with_sct: bool) -> Self {
		let root = CertifiedIssuer::self_signed(ca_params("Test TLS root"), p384()).expect("root");
		let intermediate =
			CertifiedIssuer::signed_by(ca_params("Test TLS intermediate"), p384(), &root)
				.expect("intermediate");
		let intermediate_spki_hash = crate::cert::CertInfo::parse(intermediate.der())
			.expect("a certificate")
			.spki_hash;

		let leaf_key = p384();
		let leaf_serial = vec![0x01, 0x23, 0x45, 0x67];
		let mut params = CertificateParams::new(vec!["localhost".to_owned()]).expect("valid SAN");
		params
			.distinguished_name
			.push(DnType::CommonName, "localhost");
		params.serial_number = Some(SerialNumber::from_slice(&leaf_serial));
		params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ServerAuth];
		params.use_authority_key_identifier_extension = true;
		if with_sct {
			params.custom_extensions = vec![sct_extension(TEST_LOG_ID, TEST_SCT_TIMESTAMP)];
		}
		let leaf = params.signed_by(&leaf_key, &intermediate).expect("leaf");

		Self {
			root: root.der().clone(),
			intermediate: intermediate.der().clone(),
			leaf: leaf.der().clone(),
			leaf_key: PrivatePkcs8KeyDer::from(leaf_key.serialize_der()).into(),
			leaf_serial,
			intermediate_spki_hash,
		}
	}

	/// A server config presenting the leaf and intermediate.
	pub fn server_config(&self) -> rustls::ServerConfig {
		rustls::ServerConfig::builder_with_provider(provider())
			.with_safe_default_protocol_versions()
			.expect("protocol versions")
			.with_no_client_auth()
			.with_single_cert(
				vec![self.leaf.clone(), self.intermediate.clone()],
				self.leaf_key.clone_key(),
			)
			.expect("server certificate")
	}

	/// A webpki verifier trusting this chain's root, to stand in for the platform verifier.
	pub fn verifier(&self) -> Arc<rustls::client::WebPkiServerVerifier> {
		let mut roots = rustls::RootCertStore::empty();
		roots.add(self.root.clone()).expect("root");
		rustls::client::WebPkiServerVerifier::builder_with_provider(Arc::new(roots), provider())
			.build()
			.expect("verifier")
	}
}

/// A certificate authority, for building chains of any shape.
pub struct Ca {
	params: CertificateParams,
	key: KeyPair,
	pub der: CertificateDer<'static>,
}

impl Ca {
	/// A self-signed root.
	pub fn root(name: &str) -> Self {
		let params = ca_params(name);
		let key = p384();
		let der = params.self_signed(&key).expect("root").der().clone();
		Self { params, key, der }
	}

	/// An intermediate under this one.
	pub fn issue_ca(&self, name: &str) -> Self {
		let params = ca_params(name);
		let key = p384();
		let der = params
			.signed_by(&key, &rcgen::Issuer::from_params(&self.params, &self.key))
			.expect("intermediate")
			.der()
			.clone();
		Self { params, key, der }
	}

	/// `other`'s subject and key, certified by this one instead: a cross-sign.
	pub fn cross_sign(&self, other: &Ca) -> CertificateDer<'static> {
		other
			.params
			.signed_by(
				&other.key,
				&rcgen::Issuer::from_params(&self.params, &self.key),
			)
			.expect("cross-sign")
			.der()
			.clone()
	}

	/// A leaf for `localhost` under this one, with an SCT from [`TEST_LOG_ID`] if `sct` is set,
	/// and naming this issuer's key in an authority key identifier if `aki` is set.
	pub fn issue_leaf(&self, serial: &[u8], sct: bool, aki: bool) -> CertificateDer<'static> {
		let mut params = CertificateParams::new(vec!["localhost".to_owned()]).expect("valid SAN");
		params
			.distinguished_name
			.push(DnType::CommonName, "localhost");
		params.serial_number = Some(SerialNumber::from_slice(serial));
		params.use_authority_key_identifier_extension = aki;
		if sct {
			params.custom_extensions = vec![sct_extension(TEST_LOG_ID, TEST_SCT_TIMESTAMP)];
		}
		params
			.signed_by(
				&p384(),
				&rcgen::Issuer::from_params(&self.params, &self.key),
			)
			.expect("leaf")
			.der()
			.clone()
	}

	/// SHA-256 of this authority's public key, as CRLite takes it.
	pub fn spki_hash(&self) -> [u8; 32] {
		crate::cert::CertInfo::parse(&self.der)
			.expect("a certificate")
			.spki_hash
	}
}

/// An embedded SCT list extension holding one SCT. Its signature is junk, which nothing here
/// checks.
fn sct_extension(log_id: [u8; 32], timestamp: u64) -> CustomExtension {
	let mut sct = vec![0u8];
	sct.extend_from_slice(&log_id);
	sct.extend_from_slice(&timestamp.to_be_bytes());
	sct.extend_from_slice(&[0, 0]); // no extensions
	sct.extend_from_slice(&[4, 3, 0, 1, 0]); // ecdsa_secp256r1_sha256, one byte of signature

	let mut list = Vec::new();
	list.extend_from_slice(&(sct.len() as u16 + 2).to_be_bytes());
	list.extend_from_slice(&(sct.len() as u16).to_be_bytes());
	list.extend(sct);

	let mut content = vec![0x04, list.len() as u8];
	content.extend(list);
	CustomExtension::from_oid_content(&[1, 3, 6, 1, 4, 1, 11129, 2, 4, 2], content)
}

/// A OneCRL record naming `cert` by issuer and serial number.
pub fn onecrl_issuer_serial(cert: &CertificateDer<'_>) -> Value {
	let info = crate::cert::CertInfo::parse(cert).expect("a certificate");
	json!({
		"id": hex::encode(Sha256::digest(cert)),
		"last_modified": 1,
		"issuerName": BASE64_STANDARD.encode(&info.issuer),
		"serialNumber": BASE64_STANDARD.encode(&info.serial),
		"enabled": true,
	})
}

/// A OneCRL record naming `cert` by subject and public key hash.
pub fn onecrl_subject_key(cert: &CertificateDer<'_>) -> Value {
	let info = crate::cert::CertInfo::parse(cert).expect("a certificate");
	json!({
		"id": format!("sk-{}", hex::encode(Sha256::digest(cert))),
		"last_modified": 1,
		"subject": BASE64_STANDARD.encode(&info.subject),
		"pubKeyHash": BASE64_STANDARD.encode(info.spki_hash),
	})
}

/// A CRLite filter listing `revoked` (issuer key hash, serial) pairs and stating `not_revoked`
/// ones, covering every timestamp from [`TEST_LOG_ID`].
///
/// A filter answers correctly only for the certificates it was built over, as a real one is built
/// over every logged certificate, so whatever a test queries belongs in one list or the other.
#[cfg(feature = "crlite")]
pub fn crlite_filter(
	revoked: &[([u8; 32], Vec<u8>)],
	not_revoked: &[([u8; 32], Vec<u8>)],
) -> Vec<u8> {
	use clubcard::builder::{ApproximateRibbon, ClubcardBuilder, ExactRibbon};
	use clubcard_crlite::{
		CRLiteClubcard, CRLiteCoverage, CRLiteQuery, Encoding, IssuerSpkiHash,
		builder::CRLiteBuilderItem,
	};

	let coverage = CRLiteCoverage::from_mozilla_ct_logs_json(
		json!([{
			"LogID": BASE64_STANDARD.encode(TEST_LOG_ID),
			"MinTimestamp": 0,
			"MaxTimestamp": u64::MAX / 2,
			"MMD": 0,
			"MinEntry": 0,
		}])
		.to_string()
		.as_bytes(),
	);

	type Serials = Vec<Vec<u8>>;
	let mut by_issuer: HashMap<[u8; 32], (Serials, Serials)> = HashMap::new();
	for (issuer, serial) in revoked {
		by_issuer.entry(*issuer).or_default().0.push(serial.clone());
	}
	for (issuer, serial) in not_revoked {
		by_issuer.entry(*issuer).or_default().1.push(serial.clone());
	}

	let item = |issuer: &[u8; 32], serial: &Vec<u8>, revoked: bool| {
		if revoked {
			CRLiteBuilderItem::revoked(IssuerSpkiHash(*issuer), serial.clone())
		} else {
			CRLiteBuilderItem::not_revoked(IssuerSpkiHash(*issuer), serial.clone())
		}
	};

	// The exact ribbons read the approximate ones, so those are collected first.
	let mut builder = ClubcardBuilder::new();
	let mut approx = Vec::new();
	for (issuer, (revoked, not_revoked)) in &by_issuer {
		let mut ribbon = builder.new_approx_builder(issuer);
		ribbon.set_universe_size(revoked.len() + not_revoked.len());
		for serial in revoked {
			ribbon.insert(item(issuer, serial, true));
		}
		approx.push(ApproximateRibbon::from(ribbon));
	}
	builder.collect_approx_ribbons(approx);

	let mut exact = Vec::new();
	for (issuer, (revoked, not_revoked)) in &by_issuer {
		let mut ribbon = builder.new_exact_builder(issuer);
		for serial in revoked {
			ribbon.insert(item(issuer, serial, true));
		}
		for serial in not_revoked {
			ribbon.insert(item(issuer, serial, false));
		}
		exact.push(ExactRibbon::from(ribbon));
	}
	builder.collect_exact_ribbons(exact);

	CRLiteClubcard::from(builder.build::<CRLiteQuery<'_>>(coverage, ()))
		.to_bytes(Encoding::V4)
		.expect("encoding a filter")
}

/// A `cert-revocations` record for `filter`, served from `location`.
pub fn crlite_record(id: &str, filter: &[u8], location: &str, incremental: bool) -> Value {
	json!({
		"id": id,
		"last_modified": 1,
		"channel": "default",
		"incremental": incremental,
		"effectiveTimestamp": 1,
		"attachment": {
			"hash": hex::encode(Sha256::digest(filter)),
			"size": filter.len(),
			"location": location,
			"filename": location,
			"mimetype": "application/octet-stream",
		},
	})
}

#[derive(Default)]
struct State {
	collections: HashMap<String, Collection>,
	attachments: HashMap<String, Vec<u8>>,
	requests: Vec<String>,
	failing: bool,
}

struct Collection {
	timestamp: u64,
	records: Vec<Value>,
	signature: String,
}

/// A Remote Settings server on loopback, serving whatever collections it is given, signed by a
/// [`SigningPki`].
pub struct FakeRemoteSettings {
	addr: SocketAddr,
	state: Arc<Mutex<State>>,
}

impl FakeRemoteSettings {
	pub fn start(pki: &SigningPki) -> Self {
		let listener = TcpListener::bind("127.0.0.1:0").expect("binding loopback");
		let addr = listener.local_addr().expect("bound address");
		let state = Arc::new(Mutex::new(State::default()));
		let chain = pki.chain_pem().to_owned();

		let served = Arc::clone(&state);
		std::thread::Builder::new()
			.name("fake-remote-settings".into())
			.spawn(move || {
				for stream in listener.incoming().flatten() {
					let state = Arc::clone(&served);
					let chain = chain.clone();
					std::thread::spawn(move || serve(stream, addr, &state, &chain));
				}
			})
			.expect("spawning the fake server");

		Self { addr, state }
	}

	pub fn base_url(&self) -> Url {
		Url::parse(&format!("http://{}/v1/", self.addr)).expect("a valid URL")
	}

	/// Publish `records` as `collection` at `timestamp`, signed by `pki`.
	pub fn publish(&self, pki: &SigningPki, collection: &str, records: Vec<Value>, timestamp: u64) {
		let signature = pki.sign(&records, timestamp);
		self.publish_with_signature(collection, records, timestamp, signature);
	}

	/// Publish `records` with whatever `signature` is given, valid or not.
	pub fn publish_with_signature(
		&self,
		collection: &str,
		records: Vec<Value>,
		timestamp: u64,
		signature: String,
	) {
		self.state.lock().expect("poisoned").collections.insert(
			collection.to_owned(),
			Collection {
				timestamp,
				records,
				signature,
			},
		);
	}

	/// Serve `bytes` as the attachment at `location`.
	pub fn attach(&self, location: &str, bytes: Vec<u8>) {
		self.state
			.lock()
			.expect("poisoned")
			.attachments
			.insert(location.to_owned(), bytes);
	}

	/// Answer every request with a server error, or stop doing so.
	pub fn set_failing(&self, failing: bool) {
		self.state.lock().expect("poisoned").failing = failing;
	}

	/// The paths requested so far.
	pub fn requests(&self) -> Vec<String> {
		self.state.lock().expect("poisoned").requests.clone()
	}
}

fn serve(stream: TcpStream, addr: SocketAddr, state: &Mutex<State>, chain: &str) {
	let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
	let mut reader = BufReader::new(&stream);
	let mut request_line = String::new();
	if reader.read_line(&mut request_line).is_err() {
		return;
	}
	loop {
		let mut line = String::new();
		match reader.read_line(&mut line) {
			Ok(0) | Err(_) => return,
			Ok(_) if line == "\r\n" => break,
			Ok(_) => {}
		}
	}

	let target = request_line.split(' ').nth(1).unwrap_or("/").to_owned();
	let path = target.split('?').next().unwrap_or("/").to_owned();
	let (status, body) = respond(&path, addr, state, chain);
	state.lock().expect("poisoned").requests.push(target);

	let mut stream = stream;
	let _ = write!(
		stream,
		"HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
		body.len()
	);
	let _ = stream.write_all(&body);
}

fn respond(
	path: &str,
	addr: SocketAddr,
	state: &Mutex<State>,
	chain: &str,
) -> (&'static str, Vec<u8>) {
	let state = state.lock().expect("poisoned");
	if state.failing {
		return ("500 Internal Server Error", Vec::new());
	}

	let ok = |value: Value| ("200 OK", value.to_string().into_bytes());
	match path {
		"/v1/" => ok(json!({
			"capabilities": { "attachments": { "base_url": format!("http://{addr}/attachments/") } },
		})),
		"/v1/buckets/monitor/collections/changes/changeset" => ok(json!({
			"metadata": {},
			"timestamp": state.collections.values().map(|c| c.timestamp).max().unwrap_or(0),
			"changes": state.collections.iter().map(|(name, c)| json!({
				"id": name,
				"bucket": "security-state",
				"collection": name,
				"last_modified": c.timestamp,
				"host": "localhost",
			})).collect::<Vec<_>>(),
		})),
		"/chain.pem" => ("200 OK", chain.as_bytes().to_vec()),
		_ => {
			if let Some(location) = path.strip_prefix("/attachments/")
				&& let Some(bytes) = state.attachments.get(location)
			{
				return ("200 OK", bytes.clone());
			}
			let collection = path
				.strip_prefix("/v1/buckets/security-state/collections/")
				.and_then(|rest| rest.strip_suffix("/changeset"))
				.and_then(|name| state.collections.get(name));
			match collection {
				Some(c) => ok(json!({
					"metadata": {
						"signature": {
							"x5u": format!("http://{addr}/chain.pem"),
							"signature": c.signature,
							"mode": "p384ecdsa",
						},
					},
					"timestamp": c.timestamp,
					"changes": c.records,
				})),
				None => ("404 Not Found", Vec::new()),
			}
		}
	}
}

/// Drive a TLS handshake between `client` and `server` in memory, returning whether it was full
/// or resumed, or the client's error.
pub fn handshake(
	client: &Arc<rustls::ClientConfig>,
	server: &Arc<rustls::ServerConfig>,
) -> Result<rustls::HandshakeKind, rustls::Error> {
	let name = ServerName::try_from("localhost").expect("a valid name");
	let mut client =
		rustls::Connection::Client(rustls::ClientConnection::new(Arc::clone(client), name)?);
	let mut server = rustls::Connection::Server(rustls::ServerConnection::new(Arc::clone(server))?);

	// Enough rounds for a full handshake and the session tickets that follow it.
	for _ in 0..10 {
		transfer(&mut server, &mut client)?;
		// The server's own errors are about the client's alert, not the cause.
		let _ = transfer(&mut client, &mut server);
	}
	if client.is_handshaking() {
		return Err(rustls::Error::General("handshake did not complete".into()));
	}
	match client {
		rustls::Connection::Client(client) => Ok(client
			.handshake_kind()
			.expect("a completed handshake has a kind")),
		rustls::Connection::Server(_) => unreachable!("built as a client"),
	}
}

fn transfer(
	from: &mut rustls::Connection,
	to: &mut rustls::Connection,
) -> Result<(), rustls::Error> {
	let mut buf = Vec::new();
	while from.wants_write() {
		from.write_tls(&mut buf)
			.map_err(|err| rustls::Error::General(err.to_string()))?;
	}
	let mut slice = buf.as_slice();
	while !slice.is_empty() {
		to.read_tls(&mut slice)
			.map_err(|err| rustls::Error::General(err.to_string()))?;
		to.process_new_packets()?;
	}
	Ok(())
}
