# The platform verifier reaches its Kotlin component over JNI, which Proguard cannot see,
# so without this rule the component is removed as dead code and certificate verification
# fails at runtime.
-keep, includedescriptorclasses class org.rustls.platformverifier.** { *; }

# Faith's own initialiser is called over JNI from a ContentProvider, and the native side
# resolves it by its mangled class and method name.
-keep class name.passcod.faith.FaithModule { *; }
