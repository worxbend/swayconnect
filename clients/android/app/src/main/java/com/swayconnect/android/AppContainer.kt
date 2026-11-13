package com.swayconnect.android

import android.content.Context
import android.os.Build
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
    /** Pre-fills the device name field of the pair screen. */
    val defaultDeviceName: String,
) {
    val clientInfo = ClientInfo(platform = "android", client = "swayconnect-android/0.1.0")

    /** The wiring of the app. The other constructor exists so that JUnit can build a view model without Android. */
    constructor(context: Context) : this(
        io = Dispatchers.IO,
        transport = TlsTransport(),
        profileStore = ProfileStore(File(context.filesDir, PROFILE_FILE), KeystoreSecretsStore(context)),
        defaultDeviceName = Build.MODEL,
    )

    private companion object {
        const val PROFILE_FILE = "profile.json"
    }
}
