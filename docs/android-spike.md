# Android GPUIの依存検証

検証日: 2026-11-09。これはソースとCargo依存解決の検証結果。Android向けコンパイル、APK作成、エミュレーター・実機での起動はまだ実施していない。デスクトップの依存や実装は変更していない。

## 確認した組み合わせ

| 部分 | 固定した版・コミット | GPUI依存 |
| --- | --- | --- |
| UltraTasksデスクトップ | `gpui-kit =0.9.0` | `gpui-pre =0.3.11` |
| 公式互換モバイルホスト | [`longbridge/gpui-mobile@7548ebc4b24f00494af9976af8852c7163ded9ce`](https://github.com/longbridge/gpui-mobile/tree/7548ebc4b24f00494af9976af8852c7163ded9ce) | core / wgpuともに`=0.3.10` |
| 上記ホストの公式exampleが指定するKit | [`longbridge/gpui-kit@ae6a3fdfef416e7040e773415ac9cbd8c623e5d5`](https://github.com/longbridge/gpui-kit/tree/ae6a3fdfef416e7040e773415ac9cbd8c623e5d5)（manifestは0.8.1） | `gpui-pre =0.3.10` |

以前調べたモバイルガイドの0.3.4固定例から、フォークの実ソースは進んでいる。しかしデスクトップの0.3.11とは、まだそのまま組み合わせられない。

## 実行した確認

リポジトリ外の隔離プロジェクトで、Rust 1.99.1を使用した。モバイルホストは上記コミットのクリーンなcheckoutをpath依存として指定し、`default-features = false` にした。

```sh
cargo tree --manifest-path <probe>/Cargo.toml \
  --target aarch64-linux-android -i gpui-pre
```

**Kit 0.9.0との組み合わせは依存解決に失敗した。** Cargoは、モバイルの要求する`=0.3.10`と、Kitの要求する`=0.3.11`の競合を報告した。単なる懸念ではなく、現在のmanifestを使って再現した結果である。

比較としてKitを公式exampleと同じ`ae6a3f…`へ変更すると、依存解決は成功した。逆依存ツリーでKit / Base / Component / Mobile / Wgpuがすべて`gpui-pre 0.3.10`へ揃うことを確認した。これはビルド成功を意味しない。

比較用manifestの主要部分は次のとおり。正式な製品依存の選定ではない。

```toml
[dependencies.gpui-kit]
git = "https://github.com/longbridge/gpui-kit"
rev = "ae6a3fdfef416e7040e773415ac9cbd8c623e5d5"
default-features = false
features = ["component"]

# 実際の検証は下記コミットのクリーンcheckoutへのpath依存を使用。
[dependencies.gpui-mobile]
package = "gpui-pre-mobile"
git = "https://github.com/longbridge/gpui-mobile"
rev = "7548ebc4b24f00494af9976af8852c7163ded9ce"
default-features = false
```

## WebViewを使わないための確認

- モバイルホストのdefault featuresには`webview`を含む多くのOS機能が入る。今回の依存検証では全default featuresを無効化した。
- Android向けの選択済み依存ツリーで、WebView / WebKit / Wry / Tauri / CEFの描画パッケージが含まれないことを確認した。モバイルの`webview`機能も有効にしていない。
- **サンプルAndroidホストのJavaコードは別途整理が必要。** [`GpuiPlatformView.java`](https://github.com/longbridge/gpui-mobile/blob/7548ebc4b24f00494af9976af8852c7163ded9ce/example/android/gradle/app/src/main/java/dev/gpui/mobile/GpuiPlatformView.java)には`android.webkit.WebView`のimportと生成処理が残る。Rustのfeatureを切るだけで、コピーしたJavaソースが削除されるわけではない。
- 製品ホストは必要なActivity・IME・ライフサイクル・アクセシビリティ処理を選んで取り込み、Java/Kotlin側とAPK内容も確認する。公式example全体をそのまま製品へコピーしない。

## ホストと現在の環境

公式exampleはNativeActivityを使い、Rustの`android_main`からモバイルplatformを初期化して`Application::with_platform`へ渡す。描画はAndroid native windowとwgpu、入力にはJava/JNIの連携を持つ。[entry point](https://github.com/longbridge/gpui-mobile/blob/7548ebc4b24f00494af9976af8852c7163ded9ce/example/src/lib.rs)

exampleのGradle設定は`minSdk 26`、`compileSdk / targetSdk 34`、arm64-v8a。これをUltraTasksの対応端末や配布設定として採用済みとは扱わない。[Gradle設定](https://github.com/longbridge/gpui-mobile/blob/7548ebc4b24f00494af9976af8852c7163ded9ce/example/android/gradle/app/build.gradle.kts)

現在の作業環境にはJava 21がある。`adb`、`sdkmanager`、Android SDK / NDKの設定・実行ファイルは確認できず、接続されたAndroid実機も確認していない。大きなSDK / NDKの導入は今回行っていない。

## 次の実装で行うこと

1. デスクトップの採用版を維持し、モバイルplatform・renderer・Kitを同じGPUI系列へ揃える。まずフォークを0.3.11へ追従させる変更規模を調べ、API差分を修正する。Cargoの数字だけを書き換えたものを動作済みと扱わない。
2. SDK、NDK、Rust Android target、cargo-ndkを固定し、最小のネイティブホストをビルドする。必要なGPUI型・JNI・描画・入力が通ることを先に確認する。
3. 共通coreへ接続し、タスク入力・保存・復元を実機で確認する。日本語IME、ソフトウェアキーボード、回転、バックグラウンド復帰、TalkBack、描画負荷を記録する。

比較用の古いKitを、そのままAndroid専用の恒久的な別UI基盤として採用する決定はしていない。共通coreの開発はこの依存問題から独立して進められる。
