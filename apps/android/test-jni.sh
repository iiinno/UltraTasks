#!/usr/bin/env bash
set -euo pipefail
repo_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)"
cargo build --locked --manifest-path "$repo_dir/Cargo.toml" -p ultratasks-android
mkdir -p "$repo_dir/target/android-jni-test"
javac -d "$repo_dir/target/android-jni-test" "$repo_dir/apps/android/java/app/ultratasks/system/NativeBridge.java" "$repo_dir/apps/android/tests/BridgeSmoke.java"
java -Djava.library.path="$repo_dir/target/debug" -cp "$repo_dir/target/android-jni-test" app.ultratasks.system.BridgeSmoke

javac -d "$repo_dir/target/android-jni-test" "$repo_dir/apps/android/java/app/ultratasks/system/JobRunState.java" "$repo_dir/apps/android/tests/JobRunStateTest.java"
java -cp "$repo_dir/target/android-jni-test" app.ultratasks.system.JobRunStateTest
