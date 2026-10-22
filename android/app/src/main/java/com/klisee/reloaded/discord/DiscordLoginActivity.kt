package com.klisee.reloaded.discord

import android.annotation.SuppressLint
import android.app.Activity
import android.graphics.Color
import android.os.Bundle
import android.util.Log
import android.view.ViewGroup
import android.webkit.JavascriptInterface
import android.webkit.WebView
import android.webkit.WebViewClient
import android.widget.FrameLayout
import androidx.core.view.ViewCompat
import androidx.core.view.WindowCompat
import androidx.core.view.WindowInsetsCompat
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.TimeoutCancellationException
import kotlinx.coroutines.withTimeout

/**
 * Abre el login de Discord y lee el token de sesión que la página guarda, para que la app se
 * identifique en el Gateway como este jugador.
 *
 * Es la vía de "token de usuario": no requiere aprobación de Discord, pero se usa el token de la
 * cuenta fuera de un cliente oficial, cosa que los Términos de Discord no permiten. La pantalla
 * anterior avisa de ello.
 */
@SuppressLint("SetJavaScriptEnabled")
class DiscordLoginActivity : Activity() {

    companion object {
        private const val TAG = "DiscordRpc"
        private const val LOGIN_URL = "https://discord.com/login"

        /**
         * Lee el token que Discord guarda en `localStorage`, con un iframe de reserva por si la
         * página deja de exponerlo en el documento principal.
         */
        private val EXTRACT_JS = """
            (function() {
                function fallback() {
                    try {
                        var i = document.createElement('iframe');
                        document.body.appendChild(i);
                        setTimeout(function() {
                            try {
                                var alt = i.contentWindow.localStorage.token;
                                if (alt) { Android.onRetrieveToken(alt.slice(1, -1)); }
                                else { Android.onRetrieveToken('null'); }
                            } catch (e) { Android.onRetrieveToken('error'); }
                        }, 1000);
                    } catch (e) { Android.onRetrieveToken('error'); }
                }
                try {
                    var token = localStorage.getItem('token');
                    if (token) { Android.onRetrieveToken(token.slice(1, -1)); }
                    else { fallback(); }
                } catch (e) { fallback(); }
            })();
        """.trimIndent()

        @Volatile
        private var pending: CompletableDeferred<String>? = null

        fun newPending(): CompletableDeferred<String> {
            val deferred = CompletableDeferred<String>()
            pending = deferred
            return deferred
        }

        suspend fun awaitToken(timeoutMs: Long = 180_000L): String {
            val deferred = pending ?: throw DiscordAuthException.Cancelled()
            return try {
                withTimeout(timeoutMs) { deferred.await() }
            } catch (e: TimeoutCancellationException) {
                throw DiscordAuthException.Cancelled()
            }
        }

        fun cancelPending() {
            pending?.cancel()
        }
    }

    private var webView: WebView? = null

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)

        val web = WebView(this)
        web.settings.javaScriptEnabled = true
        web.settings.domStorageEnabled = true
        web.addJavascriptInterface(
            object {
                @Suppress("unused")
                @JavascriptInterface
                fun onRetrieveToken(token: String) = deliver(token)
            },
            "Android",
        )
        web.webViewClient = object : WebViewClient() {
            override fun onPageFinished(view: WebView, url: String?) {
                super.onPageFinished(view, url)
                // El token solo existe cuando la cuenta ya ha iniciado sesión, es decir, cuando
                // Discord ha navegado a la app.
                if (url != null && (url.contains("/channels/@me") || url.contains("/app"))) {
                    view.evaluateJavascript(EXTRACT_JS, null)
                }
            }
        }
        web.loadUrl(LOGIN_URL)

        // Android 15 draws activities edge to edge. The WebView itself ignores padding in some
        // versions, so it goes inside a container whose padding is what insets the page: the
        // Discord header then starts below the clock instead of under it.
        WindowCompat.setDecorFitsSystemWindows(window, false)
        val root = FrameLayout(this).apply { setBackgroundColor(Color.BLACK) }
        root.addView(web, FrameLayout.LayoutParams(FrameLayout.LayoutParams.MATCH_PARENT, FrameLayout.LayoutParams.MATCH_PARENT))
        setContentView(
            root,
            ViewGroup.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.MATCH_PARENT),
        )
        ViewCompat.setOnApplyWindowInsetsListener(root) { view, insets ->
            val bars = insets.getInsets(
                WindowInsetsCompat.Type.systemBars() or
                    WindowInsetsCompat.Type.displayCutout() or
                    WindowInsetsCompat.Type.ime(),
            )
            view.setPadding(bars.left, bars.top, bars.right, bars.bottom)
            insets
        }
        ViewCompat.requestApplyInsets(root)
        webView = web
    }

    override fun onDestroy() {
        // Discord leaves the session token in localStorage. It is already encrypted in
        // discord_auth.xml, so the WebView copy is wiped instead of staying on disk.
        webView?.apply {
            clearCache(true)
            clearFormData()
            clearHistory()
        }
        runCatching { android.webkit.WebStorage.getInstance().deleteAllData() }
        runCatching { android.webkit.CookieManager.getInstance().removeAllCookies(null) }
        webView?.destroy()
        webView = null
        super.onDestroy()
    }

    @Deprecated("The system back gesture closes the login and cancels it.")
    override fun onBackPressed() {
        deliverCancel()
    }

    private fun deliver(token: String?) {
        val value = token?.trim().orEmpty()
        if (value.isEmpty() || value == "null" || value == "error") return
        Log.i(TAG, "session token received (length=${value.length})")
        pending?.complete(value)
        runOnUiThread { if (!isFinishing) finish() }
    }

    private fun deliverCancel() {
        pending?.completeExceptionally(DiscordAuthException.Cancelled())
        if (!isFinishing) finish()
    }
}
