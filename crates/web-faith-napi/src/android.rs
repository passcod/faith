//! Android's trust manager lives behind the JVM, and reaching it needs handles only the Java side
//! can give.
//!
//! The React Native host loads this library with `dlopen` from a TurboModule, which hands over no
//! JNI handles at all. Faith's own Gradle module therefore loads the same library through the class
//! loader at app startup, before anything requires the addon from JavaScript, and calls the entry
//! point below with the application context.
//!
//! Until that call lands, certificate verification on Android has nothing to verify against.

use jni::errors::ThrowRuntimeExAndDefault;
use jni::objects::JObject;
use jni::{EnvUnowned, jni_mangle};

/// Hand the platform verifier the environment and context it needs.
///
/// Declared on the Kotlin side as `FaithModule.initTrustStore`. Calling it more than once is
/// harmless: the verifier keeps the handles from the first call and ignores the rest.
// spec:TLS#trust-roots
#[jni_mangle("name.passcod.faith.FaithModule")]
pub fn init_trust_store<'caller>(
	mut env: EnvUnowned<'caller>,
	_this: JObject<'caller>,
	context: JObject<'caller>,
) {
	env.with_env(|env| rustls_platform_verifier::android::init_with_env(env, context))
		.resolve::<ThrowRuntimeExAndDefault>();
}
