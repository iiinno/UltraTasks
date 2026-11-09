# UltraTasks — アーキテクチャとプラットフォーム検証

更新日: 2026-11-09

本書は初期の設計提案と一次資料の調査結果をまとめる。現在はローカルコア・SQLite保存・GPUI仮画面を実装し、Linuxで検証している。同期、Windows / Android実機の検証はまだ実施していない。依存候補の機能説明を、UltraTasksでの動作保証として扱わない。

現在の実装順とユーザー回答は[システム実装計画](implementation-plan.md)を参照する。システムを優先し、UIは後からユーザーが監修する。初期対象は個人の複数端末、外部カレンダー常時連携は後回し、自己管理サーバーは内容を読める前提とする。

## 製品として守る境界

- 最初の対象はカレンダーとToDo。WindowsとAndroidのネイティブアプリを目指す。
- RustとGPUIによる描画を基本方針とし、製品UIにWebViewを使わない。
- 日常の起動・閲覧・編集はローカルで完結させる。同期サーバーは任意で、セルフホスト可能にする。
- UIの品質、入力の自然さ、反応速度を実機で評価する。機能数やフレームワークの宣伝値で代替しない。

## 提案する構成

```text
Windows UI / Android UI
          │
          ▼
Rustのアプリケーション層（commands / queries / undo）
          │
          ▼
ドメインモデル + ローカルSQLite
          │
          └── 任意の同期アダプター ── セルフホスト可能な同期サーバー

将来のAI / MCP ── 同じアプリケーション層
```

SQLiteは初期実装へ導入済み。画面の描画にネットワーク応答を必要とさせず、ディスク処理と検索を専用スレッドへ配置している。単一端末の保存と再起動後の復元を検証し、その後に端末間同期を追加する。

### Task / Event / TimeBlock

| モデル | 意味 | 初期に考える属性 |
| --- | --- | --- |
| Task | 完了させる仕事 | ID、タイトル、完了状態、期限、所属リスト |
| Event | 時間または日付が決まった予定 | ID、タイトル、開始・終了、終日区分、タイムゾーン |
| TimeBlock | Taskに取り組むために確保した時間 | ID、Task ID、開始・終了、タイムゾーン |

期限は作業開始時刻と分ける。Taskをカレンダーに置く操作ではTimeBlockを作成し、Taskそのものを複製しない。一つのTaskに複数のTimeBlockを持てる構成を候補とする。Task完了時に既存の作業枠をどう見せるかはUI仕様として決め、暗黙に削除しない。

終日予定は日付として、時刻付き予定は瞬間と表示・繰り返しに必要なタイムゾーン情報を区別して扱う。日跨ぎ、夏時間、繰り返し予定の例外は保存形式を確定する前に検証する。繰り返しと同期の詳細方式は未決定であり、初期からCRDTなどを採用済みとして扱わない。

## WindowsのUI候補

第一候補は[GPUI Kit](https://github.com/longbridge/gpui-kit)。旧リポジトリ名は `longbridge/gpui-component`。調査時点の最新リリースは[v0.9.0](https://github.com/longbridge/gpui-kit/releases/tag/v0.9.0)で、2026-11-02公開。`gpui-kit` が、振る舞いを持つ `gpui-base` と装飾済みの `gpui-component` をまとめる。

入力・フォーカス・メニュー・ポップオーバーなどは既存部品の利用を優先し、UltraTasksのトークンと画面構成を上に重ねる。[公式Design Guides](https://gpui-kit.com/docs/design-guides/)を参照し、独自プリミティブを増やす前に既存部品で必要な操作を成立させる。

[Calendar](https://github.com/longbridge/gpui-kit/blob/main/crates/component/src/time/calendar.rs)は日付選択の部品であり、週の時間軸・予定の重なり配置・ドラッグでの時刻変更が完成しているという意味ではない。この予定配置部分はUltraTasks固有の実装として切り出す。[DatePicker](https://github.com/longbridge/gpui-kit/blob/main/crates/component/src/time/date_picker.rs)の現在のmainには時刻入力との連携があるが、採用するタグでのAPIを別途確認する。

## Androidの成立性

[GPUI Kit Mobile Guide](https://gpui-kit.com/docs/mobile/)はAndroidを実験的な対応と位置付ける。限定されたチャット画面の部品には検証実績があるとされる一方、完全なアプリレイアウトとカレンダーの成立性は確認できない。Android用ホスト・描画経路は存在するが、UltraTasksの完成品質を保証するものではない。

依存の組み合わせに具体的な未解決点がある。

| 資料・候補 | 確認できたGPUI依存 |
| --- | --- |
| GPUI Kit 0.9.0 / 調査時点のmain | `gpui-pre =0.3.11` |
| 公式Mobile Guideの固定例 | `gpui-pre =0.3.4`、Kit rev `7d9efcd2069f9eaa6eb3ba6345aac4aa7d87c9f7`、mobile rev `0b882efdac7f524e0bb0b1d4c886b2aa752f9f20` |
| 上流 `itsbalamurali/gpui-mobile` の調査時点のmain | `gpui-pre =0.3.6` |

公式ガイドも、固定例と現在のKitを単に組み合わせると異なるGPUI型が混在することを指摘している。core・renderer・platform・Kitを同じ互換系列に揃え、実際にビルド・実行できた組み合わせをCargo.lockと記録に固定する。最新版同士で動くとは仮定しない。

参照: [Kit workspace manifest](https://github.com/longbridge/gpui-kit/blob/main/Cargo.toml)、[公式互換フォーク](https://github.com/longbridge/gpui-mobile)、[上流mobile manifest](https://github.com/itsbalamurali/gpui-mobile/blob/main/Cargo.toml)。mainのリンクは変更されるため、導入時には採用コミットのリンクを追加する。

## 最初の検証スパイク

最小の縦断画面として、タスク一覧・日本語入力欄・日付選択・一日分の時間軸を用意する。完成画面の機能数を増やす前に、次の証拠を残す。

1. **依存とビルド**: Windows release build、Android APK作成、使用したRust・SDK・NDK・依存コミットを記録する。コンパイル成功と実機成功を別々に記す。
2. **入力**: 日本語IMEの未確定文字、変換候補、確定・取消、選択範囲、コピー・貼り付け、Undoを試す。Androidではソフトウェアキーボード表示、戻る操作、入力欄へのフォーカス移動も確認する。
3. **アクセシビリティ**: WindowsのNarrator、AndroidのTalkBackで名前・役割・状態・読み上げ順・操作が届くかを確認する。ライブラリ側のAccessKit対応だけで合格にしない。
4. **ライフサイクル**: ウィンドウのリサイズ、DPI変更、Androidの回転・バックグラウンド復帰・プロセス終了後の復元を確認する。描画サーフェス再生成と編集中の内容保持を含む。
5. **予定操作**: ドラッグ中の表示、キャンセル、スナップ、重なった予定、日跨ぎ、タッチでの操作を試す。ドラッグ以外の編集手段も用意する。
6. **性能**: 端末名・OS・ビルド種別・データ件数を添え、起動時間、操作から描画までの遅延、フレーム時間、メモリ、アイドル時の負荷を測る。シミュレーター値を実機性能として扱わない。

成果物は短い操作録画、同一条件のスクリーンショット、測定ログ、未解決項目。Androidで必要な入力・アクセシビリティ・ライフサイクルが成立しなければ、問題箇所と修正規模を記録してUI実装の方針を再評価する。WebViewへの自動的な置換は行わない。

## WebViewを含めない依存確認

GPUI Kitには別のWebViewクレートがあり、上流 `gpui-mobile` のdefault featuresにも `webview` が含まれる。ネイティブ描画ライブラリを選ぶだけでは条件を満たしたことにならない。

導入時に不要なdefault featuresを無効化し、`cargo tree -e features --target <target>` と実際のコード・Androidパッケージを確認する。WebView用クレートやAPIが製品の依存・実行経路に入っていないことを記録する。スマートフォンのIMEや通知など、OSとのネイティブ連携はこの制約と両立する。

## ライセンスの境界

- GPUIとGPUI KitのソフトウェアはApache-2.0。Kitの資料の文章・図にはCC BY 4.0という別の条件がある。[Kitの説明](https://github.com/longbridge/gpui-kit#license)
- Zed本体のUIクレートはGPL-3.0-or-later。GPUIと同じライセンスとしてコードを持ち込まない。[Zed UI manifest](https://github.com/zed-industries/zed/blob/main/crates/ui/Cargo.toml)、[Zed Software Overview](https://zed.dev/software-overview)
- 上流 `gpui-mobile` のmanifestは `GPL-3.0-or-later OR AGPL-3.0-or-later OR Apache-2.0`。採用コミットのファイル・依存の条件も確認し、出典と必要な通知を残す。

## 将来のAI / MCP

AIとMCPはカレンダー・ToDoの操作品質を確立した後に追加する。予定検索、自然文からの入力、作業時間の提案を候補とする。

UI・AI・MCPは同じcommands / queriesを通す。AIからSQLiteへ直接書き込ませず、検証・対象範囲・変更履歴・Undoを迂回させない。複数項目の変更はプレビューできる操作のまとまりとして扱う。外部クライアントや同期との競合がある場合、Undoは他端末の変更を無条件に巻き戻さない設計が必要になる。

モデル提供元、ローカルモデル、MCPの通信方式、同期プロトコルは未選定。現段階では外部AIサービスへの接続・資格情報の設定を行っていない。
