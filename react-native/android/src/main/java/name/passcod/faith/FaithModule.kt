package name.passcod.faith

import android.content.Context

/**
 * The bridge between Android's trust manager and Faith's TLS layer.
 *
 * The React Native host loads Faith's library with `dlopen` and so hands it no JNI handles.
 * Loading the same library through the class loader here gives the JVM a library to resolve
 * [initTrustStore] against, and the content provider that calls it supplies the context.
 *
 * `System.loadLibrary` rather than SoLoader: this runs before the application's own
 * `onCreate`, which is where React Native initialises SoLoader, so SoLoader is not ready
 * yet. The platform loader needs no initialisation.
 */
internal object FaithModule {
    init {
        System.loadLibrary("faith")
    }

    /**
     * Give the certificate verifier the JVM handles it needs.
     *
     * Implemented in Rust. Calling it more than once is harmless; the verifier keeps the
     * handles from the first call.
     */
    external fun initTrustStore(context: Context)
}
