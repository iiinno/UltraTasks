#!/usr/bin/env bash
# Minimal system-host debug APK; this does not build the GPUI Android renderer.
set -euo pipefail
repo_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)"
: "${ANDROID_SDK_ROOT:?Set ANDROID_SDK_ROOT to the installed Android SDK}"
android_ndk_dir="${ANDROID_NDK_HOME:-$ANDROID_SDK_ROOT/ndk/28.0.13004108}"
android_tools_dir="$ANDROID_SDK_ROOT/build-tools/35.0.0"
android_jar="$ANDROID_SDK_ROOT/platforms/android-35/android.jar"
android_build_dir="$repo_dir/target/android-system"
python3 - "$android_build_dir" <<'PY'
from pathlib import Path
import shutil,sys
build=Path(sys.argv[1])
for name in ['classes','dex','apk']:
    shutil.rmtree(build/name,ignore_errors=True)
PY
mkdir -p "$android_build_dir/classes" "$android_build_dir/dex" "$android_build_dir/apk/lib/arm64-v8a"

# Never leak the desktop sysroot into Android's bundled SQLite compilation.
unset CPATH C_INCLUDE_PATH CPLUS_INCLUDE_PATH LIBRARY_PATH PKG_CONFIG_SYSROOT_DIR
export CC_aarch64_linux_android="$android_ndk_dir/toolchains/llvm/prebuilt/linux-x86_64/bin/aarch64-linux-android26-clang"
export AR_aarch64_linux_android="$android_ndk_dir/toolchains/llvm/prebuilt/linux-x86_64/bin/llvm-ar"
export CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER="$CC_aarch64_linux_android"
cargo build --locked --manifest-path "$repo_dir/Cargo.toml" -p ultratasks-android --target aarch64-linux-android
cp "$repo_dir/target/aarch64-linux-android/debug/libultratasks_android.so" "$android_build_dir/apk/lib/arm64-v8a/"
python3 "$repo_dir/apps/android/fetch-dependencies.py" "$android_build_dir/dependencies"

# Java is deliberately only a system/permissions host, with no WebView or UI kit.
mapfile -t android_sources < <(find "$repo_dir/apps/android/java" -name '*.java' -print | sort)
javac -source 8 -target 8 -Xlint:all,-options -bootclasspath "$android_jar:$android_tools_dir/core-lambda-stubs.jar" -d "$android_build_dir/classes" "${android_sources[@]}"
mapfile -t android_classes < <(find "$android_build_dir/classes" -name '*.class' -print | sort)
# SDK35's bundled D8 only understands Kotlin2.1 metadata. The verifier's official
# runtime is Kotlin2.2; pin the first compatible D8/R8 rather than stripping it.
if ! java -cp "$android_build_dir/dependencies/r8-8.10.21.jar" com.android.tools.r8.D8 --lib "$android_jar" --min-api 26 --output "$android_build_dir/dex" "${android_classes[@]}" "$android_build_dir/dependencies/classes.jar" "$android_build_dir/dependencies/kotlin-stdlib-2.2.10.jar" "$android_build_dir/dependencies/annotations-13.0.jar" > "$android_build_dir/d8.log" 2>&1; then
    head -c 4000 "$android_build_dir/d8.log"
    exit 1
fi
python3 - "$android_build_dir/d8.log" <<'PY'
from pathlib import Path
import sys
log=Path(sys.argv[1]).read_text()
if 'Warning:' in log or 'malformed' in log or 'Error:' in log:
    raise SystemExit('D8 emitted a diagnostic requiring review:\n'+log[:4000])
PY
cp "$android_build_dir/dex/classes.dex" "$android_build_dir/apk/"
mkdir -p "$android_build_dir/apk/assets/licenses"
cp "$repo_dir/apps/android/licenses/"* "$android_build_dir/apk/assets/licenses/"
"$android_tools_dir/aapt2" compile --dir "$android_build_dir/dependencies/res" -o "$android_build_dir/resources.zip"
"$android_tools_dir/aapt2" link -I "$android_jar" --manifest "$repo_dir/apps/android/AndroidManifest.xml" -o "$android_build_dir/unsigned.apk" "$android_build_dir/resources.zip"
python3 - "$android_build_dir" <<'PY'
import pathlib,sys,zipfile
build=pathlib.Path(sys.argv[1])
with zipfile.ZipFile(build/'unsigned.apk','a',compression=zipfile.ZIP_STORED) as archive:
    for file in sorted((build/'apk').rglob('*')):
        if file.is_file(): archive.write(file,str(file.relative_to(build/'apk')))
PY
"$android_tools_dir/zipalign" -f -P 16 4 "$android_build_dir/unsigned.apk" "$android_build_dir/aligned.apk"
if [[ ! -f "$android_build_dir/debug.keystore" ]]; then
    keytool -genkeypair -keystore "$android_build_dir/debug.keystore" -storepass android -keypass android -alias androiddebugkey -dname 'CN=Android Debug,O=Android,C=US' -keyalg RSA -keysize 2048 -validity 3650 >/dev/null 2>&1
fi
"$android_tools_dir/apksigner" sign --ks "$android_build_dir/debug.keystore" --ks-pass pass:android --key-pass pass:android --out "$android_build_dir/ultratasks-system-debug.apk" "$android_build_dir/aligned.apk"
"$android_tools_dir/apksigner" verify --verbose "$android_build_dir/ultratasks-system-debug.apk"
"$android_tools_dir/zipalign" -c -P 16 4 "$android_build_dir/ultratasks-system-debug.apk"
printf '%s\n' "$android_build_dir/ultratasks-system-debug.apk"
