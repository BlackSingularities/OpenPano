"""Patch the generated Android project (after `tauri android init`) so Panoramer appears in the
system Share / Open with sheet for images. Shared files are copied by MainActivity into
<cacheDir>/shared and picked up by the `take_shared_files` command.

Usage: python3 scripts/patch_android.py src-tauri/gen/android
"""

import pathlib
import re
import sys

root = pathlib.Path(sys.argv[1])

FILTERS = """
            <intent-filter>
                <action android:name="android.intent.action.SEND" />
                <category android:name="android.intent.category.DEFAULT" />
                <data android:mimeType="image/*" />
            </intent-filter>
            <intent-filter>
                <action android:name="android.intent.action.SEND_MULTIPLE" />
                <category android:name="android.intent.category.DEFAULT" />
                <data android:mimeType="image/*" />
            </intent-filter>
            <intent-filter>
                <action android:name="android.intent.action.VIEW" />
                <category android:name="android.intent.category.DEFAULT" />
                <data android:mimeType="image/*" />
            </intent-filter>
"""

manifest = root / "app/src/main/AndroidManifest.xml"
m = manifest.read_text(encoding="utf-8")
if "android.intent.action.SEND" not in m:
    i = m.index("</activity>")
    m = m[:i] + FILTERS + "        " + m[i:]
    manifest.write_text(m, encoding="utf-8")
    print("manifest patched")
# In-app updates: the downloaded APK is installed through PackageInstaller (AppUpdater.kt).
m = manifest.read_text(encoding="utf-8")
if "REQUEST_INSTALL_PACKAGES" not in m:
    m = m.replace("<application", '<uses-permission android:name="android.permission.REQUEST_INSTALL_PACKAGES" />\n\n    <application', 1)
if "android.permission.INTERNET" not in m:
    m = m.replace("<application", '<uses-permission android:name="android.permission.INTERNET" />\n    <application', 1)
manifest.write_text(m, encoding="utf-8")

KOTLIN = """
    private var appUpdater: AppUpdater? = null

    override fun onWebViewCreate(webView: android.webkit.WebView) {
        super.onWebViewCreate(webView)
        appUpdater = AppUpdater(this, webView, "https://redzikowski.com/api/projects/openpano/asset/")
        webView.addJavascriptInterface(appUpdater!!, "AppUpdater")
    }

    override fun onDestroy() {
        appUpdater?.dispose()
        super.onDestroy()
    }

    override fun onResume() {
        super.onResume()
        appUpdater?.onResume()
        panoramerTakeShared(intent)
    }

    override fun onNewIntent(intent: android.content.Intent) {
        super.onNewIntent(intent)
        setIntent(intent)
        panoramerTakeShared(intent)
    }

    @Suppress("DEPRECATION")
    private fun panoramerTakeShared(intent: android.content.Intent?) {
        if (intent == null || intent.getBooleanExtra("panoramer_done", false)) return
        val uris = ArrayList<android.net.Uri>()
        when (intent.action) {
            android.content.Intent.ACTION_SEND ->
                (intent.getParcelableExtra<android.os.Parcelable>(android.content.Intent.EXTRA_STREAM) as? android.net.Uri)?.let { uris.add(it) }
            android.content.Intent.ACTION_SEND_MULTIPLE ->
                intent.getParcelableArrayListExtra<android.os.Parcelable>(android.content.Intent.EXTRA_STREAM)
                    ?.forEach { (it as? android.net.Uri)?.let { u -> uris.add(u) } }
            android.content.Intent.ACTION_VIEW -> intent.data?.let { uris.add(it) }
        }
        intent.putExtra("panoramer_done", true)
        if (uris.isEmpty()) return
        val dir = java.io.File(cacheDir, "shared")
        dir.mkdirs()
        for ((i, uri) in uris.withIndex()) {
            try {
                var name = "shared_$i.jpg"
                contentResolver.query(uri, null, null, null, null)?.use { c ->
                    val col = c.getColumnIndex(android.provider.OpenableColumns.DISPLAY_NAME)
                    if (col >= 0 && c.moveToFirst()) c.getString(col)?.let { name = it }
                }
                name = name.replace(Regex("[\\\\\\\\/:*?\\"<>|]"), "_")
                contentResolver.openInputStream(uri)?.use { input ->
                    java.io.File(dir, name).outputStream().use { input.copyTo(it) }
                }
            } catch (e: Exception) {
                android.util.Log.w("Panoramer", "share import failed: $uri", e)
            }
        }
    }
"""

acts = list((root / "app/src/main/java").rglob("MainActivity.kt"))
if not acts:
    sys.exit("MainActivity.kt not found")
act = acts[0]
k = act.read_text(encoding="utf-8")
if "panoramerTakeShared" not in k:
    m2 = re.search(r"class MainActivity\s*:\s*TauriActivity\(\)\s*(\{)?", k)
    if not m2:
        sys.exit("unexpected MainActivity.kt layout")
    if m2.group(1):
        end = k.rindex("}")
        k = k[:end] + KOTLIN + k[end:]
    else:
        k = k[: m2.end()] + " {" + KOTLIN + "}\n" + k[m2.end():]
    act.write_text(k, encoding="utf-8")
    print("MainActivity patched:", act)

pkg = re.search(r"(?m)^package\s+([\w.]+)", k).group(1)
upd = (pathlib.Path(__file__).resolve().parent / "AppUpdater.kt").read_text(encoding="utf-8")
(act.parent / "AppUpdater.kt").write_text(upd.replace("__PACKAGE__", pkg), encoding="utf-8")
print("AppUpdater added:", pkg)
