#!/usr/bin/env bash
# Bootstrap a pinned Linux x86_64 SDK without relying on runner-image contents.
set -euo pipefail
android_sdk_dir="${1:-${ANDROID_SDK_ROOT:-}}"
if [[ -z "$android_sdk_dir" || $# -gt 1 ]]; then
    printf '%s\n' 'Usage: apps/android/setup-sdk.sh SDK_DIRECTORY (or set ANDROID_SDK_ROOT)' >&2
    exit 2
fi
if [[ "$(uname -s)" != Linux || "$(uname -m)" != x86_64 ]]; then
    printf '%s\n' 'This bootstrap installs the Linux x86_64 Android SDK/NDK.' >&2
    exit 2
fi
mkdir -p "$android_sdk_dir"
android_sdk_dir="$(cd -- "$android_sdk_dir" && pwd)"
export ANDROID_USER_HOME="$android_sdk_dir/.android-user"
android_sdkmanager="$android_sdk_dir/cmdline-tools/latest/bin/sdkmanager"
android_platform="$android_sdk_dir/platforms/android-35/android.jar"
android_buildtools="$android_sdk_dir/build-tools/35.0.0/apksigner"
android_clang="$android_sdk_dir/ndk/28.0.13004108/toolchains/llvm/prebuilt/linux-x86_64/bin/aarch64-linux-android26-clang"

# Reusing an already installed SDK needs neither a network request nor license
# prompts. The build itself still names exact platform/build-tools/NDK versions.
if [[ -x "$android_sdkmanager" && -f "$android_platform" && -x "$android_buildtools" && -x "$android_clang" ]]; then
    printf 'Android SDK is ready: %s\n' "$android_sdk_dir"
    exit 0
fi
for executable in java javac python3 curl; do
    command -v "$executable" >/dev/null || { printf 'Required command is missing: %s\n' "$executable" >&2; exit 1; }
done
python3 - <<'PY'
import re, subprocess
result=subprocess.run(['java','-version'],capture_output=True,text=True,check=True)
match=re.search(r'version "(\d+)',result.stderr+result.stdout)
if match is None or int(match.group(1)) < 17:
    raise SystemExit('Android bootstrap requires JDK 17 or newer')
PY

if [[ ! -x "$android_sdkmanager" ]]; then
    android_archive="$android_sdk_dir/.downloads/commandlinetools-linux-11076708_latest.zip"
    android_archive_sha='2d2d50857e4eb553af5a6dc3ad507a17adf43d115264b1afc116f95c92e5e258'
    mkdir -p "$(dirname -- "$android_archive")"
    if ! python3 - "$android_archive" "$android_archive_sha" <<'PY'
import hashlib, pathlib, sys
path=pathlib.Path(sys.argv[1])
raise SystemExit(0 if path.is_file() and hashlib.sha256(path.read_bytes()).hexdigest()==sys.argv[2] else 1)
PY
    then
        curl --fail --location --silent --show-error --retry 3 --connect-timeout 20 \
            'https://dl.google.com/android/repository/commandlinetools-linux-11076708_latest.zip' \
            --output "$android_archive.part"
        python3 - "$android_archive.part" "$android_archive_sha" <<'PY'
import hashlib, pathlib, sys
path=pathlib.Path(sys.argv[1])
if hashlib.sha256(path.read_bytes()).hexdigest()!=sys.argv[2]:
    raise SystemExit('Android command-line tools SHA-256 mismatch')
PY
        mv -- "$android_archive.part" "$android_archive"
    fi
    python3 - "$android_archive" "$android_sdk_dir" <<'PY'
from pathlib import Path
import shutil, sys, tempfile, zipfile
archive=Path(sys.argv[1]); root=Path(sys.argv[2]); parent=root/'cmdline-tools'
parent.mkdir(parents=True,exist_ok=True)
destination=parent/'latest'
if destination.exists():
    raise SystemExit('Incomplete cmdline-tools/latest exists; inspect it before replacing it')
with tempfile.TemporaryDirectory(prefix='.extract-',dir=parent) as temporary:
    with zipfile.ZipFile(archive) as contents:
        for name in contents.namelist():
            path=Path(name)
            if path.is_absolute() or '..' in path.parts:
                raise SystemExit('Unsafe path in Android SDK archive')
        contents.extractall(temporary)
    extracted=Path(temporary)/'cmdline-tools'
    for executable in (extracted/'bin').iterdir(): executable.chmod(0o755)
    shutil.move(str(extracted),str(destination))
PY
fi

# `yes` exits with SIGPIPE once sdkmanager closes stdin. Only sdkmanager's status
# determines this deliberate license-acceptance pipeline's result.
(set +o pipefail; yes | "$android_sdkmanager" --sdk_root="$android_sdk_dir" --licenses >/dev/null)
"$android_sdkmanager" --sdk_root="$android_sdk_dir" \
    'platforms;android-35' 'build-tools;35.0.0' 'ndk;28.0.13004108'
for required in "$android_platform" "$android_buildtools" "$android_clang"; do
    [[ -f "$required" ]] || { printf 'SDK installation did not produce: %s\n' "$required" >&2; exit 1; }
done
printf 'Android SDK is ready: %s\n' "$android_sdk_dir"
