package name.passcod.faith

import android.content.ContentProvider
import android.content.ContentValues
import android.database.Cursor
import android.net.Uri

/**
 * Initialises Faith's TLS layer at application startup.
 *
 * This is a content provider because Android creates those before the application's own
 * `onCreate` and hands each one a context, which is exactly what the certificate verifier
 * needs and the earliest it can be had. It serves no content.
 *
 * An app that lists Faith as a dependency gets this through manifest merging, so there is
 * nothing to add to the app itself.
 */
internal class FaithInitProvider : ContentProvider() {
    override fun onCreate(): Boolean {
        val context = checkNotNull(context) { "FaithInitProvider was created without a context" }
        FaithModule.initTrustStore(context.applicationContext)
        return true
    }

    override fun query(
        uri: Uri,
        projection: Array<out String>?,
        selection: String?,
        selectionArgs: Array<out String>?,
        sortOrder: String?,
    ): Cursor? = null

    override fun getType(uri: Uri): String? = null

    override fun insert(uri: Uri, values: ContentValues?): Uri? = null

    override fun delete(uri: Uri, selection: String?, selectionArgs: Array<out String>?): Int = 0

    override fun update(
        uri: Uri,
        values: ContentValues?,
        selection: String?,
        selectionArgs: Array<out String>?,
    ): Int = 0
}
