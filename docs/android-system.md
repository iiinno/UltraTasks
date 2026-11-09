# Androidのネイティブシステムホスト

`apps/android`は共通Rust coreをJNIから呼び、OS通知・アラーム・再起動処理へ接続する最小ホスト。WebView、Web画面、別のデータ保存実装は含まない。GPUIのAndroid描画・入力の移植は[依存検証](android-spike.md)に記載した別の課題で、このAPKを完成した製品画面として扱わない。

## ビルド

確認した構成はRust 1.99.1、Android SDK 35、build-tools 35.0.0、NDK 28.0.13004108、JDK 21、D8/R8 8.10.21、arm64-v8a、minSdk 26 / targetSdk 35。NDKとRustを固定し、Android用SQLiteもNDKでビルドする。デスクトップ用のinclude/sysrootを持ち込まない。TLS検証ライブラリのKotlin 2.2に合わせてD8も固定し、SDK 35付属D8のメタデータ互換性警告を残さない。

```sh
rustup target add aarch64-linux-android
sdkmanager 'platforms;android-35' 'build-tools;35.0.0' 'ndk;28.0.13004108'
export ANDROID_SDK_ROOT=/path/to/android-sdk
apps/android/build.sh
```

SDK標準の`javac / d8 / aapt2 / zipalign / apksigner`でシステムホストの診断用debug APKを生成する。出力は`target/android-system/ultratasks-system-debug.apk`。生成するdebug署名鍵とビルド出力は`target/`内だけに置き、Gitへ保存しない。公開配布用の署名・インストーラー設定ではない。

## 接続点

`NativeBridge.dispatch`はJSON要求を共通coreへ渡す。実行はAndroidのバックグラウンド処理から行う。DBの場所はアプリ内部の`Context.getDatabasePath`で決め、IntentからDBパスやSQLを受け取らない。変更は共有Command経由で、操作IDの重複排除、更新版の照合、Undo、SQLiteトランザクションが適用される。

`ReminderRuntime.execute`はコマンド実行後に通知を再照合する。`setEnabled`がこの端末の通知有効・無効、`requestExactAlarms`が明示的にOSの正確なアラーム設定を開く接続点。通知設定・最後の配信エラーは端末のSharedPreferencesへ保存する。起動ActivityはOS権限ダイアログの入口だけを持ち、製品のUIを実装していない。

共通Command以外のUndo/Redo、JSON/ICS入出力、パスを制限したバックアップ・新しいDBへの復元、読み取り専用接続、同期・AIの接続は[API仕様](android-system-api.md)を参照する。`SystemController`が同期・AIをバックグラウンドで呼び、`CredentialStore`がAndroidKeyStoreのAES-GCM鍵で端末内の認証情報を保護する。同期先を明示設定すると、`SyncJobService`がネットワーク利用可能時の定期同期をOSへ登録する。Doze中の実行時刻を保証する仕組みではない。

TLSは`SystemApplication`で`rustls-platform-verifier`をAndroid Application Contextへ初期化し、OSの証明書検証器へ接続する。AAR 0.2.0、Kotlin stdlib 2.2.10、JetBrains annotations 13.0を[依存固定ファイル](../apps/android/dependencies.json)のURL・SHA-256で照合する。AARとCargo.lockの対応版が変わったらビルドを止める。証明書検証を無効化しない。付属のnetwork-security-configは通常の平文通信を禁止し、公式ライブラリが列挙する証明書失効リスト配布先だけを例外にする。アプリの同期・AIエンドポイントのHTTPS検証とは独立している。

RustのJNI境界は`jni 0.22`の`EnvUnowned::with_env`を利用し、エラーとpanicをJNI呼出内で処理する。危険な生ポインター処理は書かず、エクスポートするABI名の`no_mangle`属性だけに局所的なunsafe lint例外を設ける。

## 実行した検証と残り

- Linux上でAndroid ARM64のRust共有ライブラリをコンパイルし、Javaホストをdexへ変換、APKを作成した。
- APK v2 / v3署名検証と16KiB zip alignment検証に成功した。ネイティブELFのLOADセグメントは16KiB境界。
- Java 21の実JVMからJNI共有ライブラリを読み込み、日本語タスク作成、同じ要求の再送、SQLiteを再オープンした復元、通知計画、不正入力のエラーを確認した。`apps/android/test-jni.sh`で再実行できる。
- Android端末・エミュレーターでのインストール、アラーム実配信、強制終了・Doze・再起動・端末メーカーの制限は未検証。
- GPUI描画、日本語IME、TalkBack、画面回転・復帰はこのシステムホストの検証対象外。Androidの完成品や性能達成を主張しない。

通知の時刻・権限・取得猶予は[通知仕様](notifications.md)を参照する。
