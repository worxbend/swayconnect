package com.swayconnect.android

import android.content.Context
import android.os.Build
import androidx.compose.ui.graphics.ImageBitmap
import com.swayconnect.android.data.BitmapIconDecoder
import com.swayconnect.android.data.DesktopCache
import com.swayconnect.android.data.IconCache
import com.swayconnect.android.data.KeystoreSecretsStore
import com.swayconnect.android.data.ProfileStore
import com.swayconnect.android.net.TlsTransport
import com.swayconnect.protocol.ClientInfo
import com.swayconnect.protocol.Transport
import java.io.File
import kotlinx.coroutines.CoroutineDispatcher
import kotlinx.coroutines.Dispatchers

/** Manual dependency wiring. One instance per view model. */
class AppContainer(
    /** Runs every blocking call: sockets, the profile file and the Keystore. */
    val io: CoroutineDispatcher,
    val transport: Transport,
    val profileStore: ProfileStore,
    /** The last desktop state, shown dimmed before the first connection. */
    val desktopCache: DesktopCache,
    /** Pre-fills the device name field of the pair screen. */
    val defaultDeviceName: String,
    /** App icons by hash. Null in JVM tests, which have no bitmap decoder. */
    val iconCache: IconCache<ImageBitmap>? = null,
) {
    val clientInfo = ClientInfo(platform = "android", client = "swayconnect-android/0.1.0")

    /** The wiring of the app. The other constructor exists so that JUnit can build a view model without Android. */
    constructor(context: Context) : this(
        io = Dispatchers.IO,
        transport = TlsTransport(),
        profileStore = ProfileStore(File(context.filesDir, PROFILE_FILE), KeystoreSecretsStore(context)),
        desktopCache = DesktopCache(File(context.filesDir, DESKTOP_CACHE_FILE)),
        defaultDeviceName = Build.MODEL,
        iconCache = IconCache(File(context.cacheDir, ICON_DIR), BitmapIconDecoder),
    )

    private companion object {
        const val PROFILE_FILE = "profile.json"
        const val DESKTOP_CACHE_FILE = "last_desktop.json"
        const val ICON_DIR = "icons"
    }
}
