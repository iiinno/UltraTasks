# Linear UIの参照記録

2026-11-12取得の公開資料と、UltraTasksの画面実装の対応を記録する。利用者の指定に従い、Linearの構成・寸法・部品の状態をGPUIで再実装する。現在の画面仕様は[DESIGN.md](../DESIGN.md)にある。

本体の公開静的コード、起動shell、ホームページのデモ、公式ドキュメントは別の根拠である。下表の「採用値」はRustに指定した値を示す。認証後のLinear画面と新しいネイティブUIの同等性は未確認であり、CSSの定義を描画結果の実測値とは扱わない。

## 公開資料と採用値

| 対象 | 公開URL・識別子 | UltraTasksの採用値・対応 | 確認範囲 |
| --- | --- | --- | --- |
| サイドバー | [startup-entry.CRUNKDWa.js](https://static.linear.app/client/assets/startup-entry.CRUNKDWa.js)、`Q.prototype.sidebarWidth`。[WorkbenchSidebar.BZPhlcl5.js](https://static.linear.app/client/assets/WorkbenchSidebar.BZPhlcl5.js) | 幅244px、行高28px、上下margin各1px、角丸8px | 本体の静的定義。Linear側の通常幅の範囲192〜330pxは、UltraTasksの可変幅機能としては採用していない |
| railと主領域 | [NavigationRailLayout.ice_k7Bz.js](https://static.linear.app/client/assets/NavigationRailLayout.ice_k7Bz.js)、[startup-root.Dl_Z8T-y.js](https://static.linear.app/client/assets/startup-root.Dl_Z8T-y.js)の `kj` | rail 48px、主領域の上下・右余白8px、角丸12px | 公開レイアウトを参照。900px未満でrailへ切り替える条件はUltraTasksの実装値 |
| 主ヘッダー | [startup-root.Dl_Z8T-y.js](https://static.linear.app/client/assets/startup-root.Dl_Z8T-y.js)の `mainHeaderHeight` と、インストール済みアプリの表示 | 高さ44px | `44 - thinPixel` と実画面を併用。汎用headerの上書きなし既定57pxは採用しない |
| subheader | [ContentViewSubheader.DnImr3X9.js](https://static.linear.app/client/assets/ContentViewSubheader.DnImr3X9.js)と本体の `subHeaderHeight` | 一覧の操作帯を44pxで指定し、状態pillを配置 | 主ヘッダーと別の領域として扱う |
| 一覧 | [IssueList.CFT1-SsJ.js](https://static.linear.app/client/assets/IssueList.CFT1-SsJ.js)、[ListCell.U7XnMyFw.js](https://static.linear.app/client/assets/ListCell.U7XnMyFw.js)、`kj.listIssueRowHeight`・`listHeaderHeight` | 固定行高44px、グループ見出し36px。タイトルと属性を横並びにする | 本体の静的定義と行構造を参照。GPUIの既存 `list`・`ListState`、TaskCache、選択とページングは維持 |
| 詳細 | [IssueDetailsPaneSidebar.CvxZP-1H.js](https://static.linear.app/client/assets/IssueDetailsPaneSidebar.CvxZP-1H.js)、[本体CSS](https://static.linear.app/client/assets/style-5dYGN5Zf.css)の `sx-8w46s4`・`sx-5s0vsj` | 属性幅280〜400px、列間隔16〜56px、本文最大80ch。上padding21.5px、本文上margin32px・小幅時18px | CSSのclampを `available_width` の式へ対応。900px未満で縦配置する条件は詳細領域の幅に対する実装値 |
| 作成 | [Creating issues](https://linear.app/docs/creating-issues)、[FastIssueCreateEditor.CZReDLse.js](https://static.linear.app/client/assets/FastIssueCreateEditor.CZReDLse.js)、本体CSSの `sx-c42dh1` | 幅750px・角丸21px。上位置は参照画面の上部帯42pxと13vhの余白を合わせる。上部に追加先picker、枠なし18px/600のタイトル、右端のpill CTA | 実アプリの作成画面と公開部品の構造を参照。操作結果と対象SHAは検証記録に記載 |
| 起点付き操作 | [Display options](https://linear.app/docs/display-options) | 絞り込みと時間割の日付をKit Popoverへ対応 | 操作の構成を参照。384px・344pxの幅とoffset 6pxはUltraTasksの指定値で、Linearの実測値ではない |
| デモの文字と配置 | [Linearホームページ](https://linear.app/)、`Mmx1Wq_sidebar`・`bVIB3G_row`・`KFZpfa_viewBody`。[Comment on issues](https://linear.app/docs/comment-on-issues) | navigationと行のweight 510、詳細タイトル20px・weight 590・行高26.6px | デモのsidebar列232px・内側の行208px・header 44pxは本体の244px・58pxへ混同しない |
| 本文フォント | [本体CSS](https://static.linear.app/client/assets/style-5dYGN5Zf.css)の `--font-size-small` とweight定義、[Inter 4.1](https://github.com/rsms/inter/releases/tag/v4.1) | 本文Inter 13px、行高19.5px、weight 450。共通controlは500 | 本体の小サイズ0.8125remと行高1.5へ対応。静的TTFのface選択とvariable 450の再現は未検証 |

詳細の属性幅は `(available_width × 0.26087 + 66.087).clamp(280, 400)`、間隔は `(available_width × 0.08696 - 55.304).clamp(16, 56)` とする。80chは本文フォントの `0` の字幅×80で求める。タイトルはviewの590指定を記録し、設計JSONのtitle 600とtracking `-0.24` をそのまま適用したとは記載しない。

## 色の出所

| 対象 | 根拠 | 採用値と限界 |
| --- | --- | --- |
| shellの面と境界 | [公開起動shell](https://linear.app/auth/desktop) | darkはsidebar `#09090a`、canvas `#121213`、border `#212224`。lightは `#efeff0`・`#f9f9fa`・`#e2e2e2`。認証後のテーマ出力は未取得 |
| darkの文字 | 同ホームページのデモ | 本文 `#e2e4e7`、補助文字 `#8a8f98` |
| darkのhoverと選択 | デモのoverlayとshellのcanvas | `#ffffff05` と `#ffffff0a` を `#121213` へsRGBで合成し、8bitへ丸めた `#171718` と `#1b1b1c`。本体の解決済みテーマ値ではない |
| elevated面 | 本体 `elevatedTheme()` のbgBase | dark LCH `[9.2325,.85,272]` をD50からsRGBへ変換し `#1a1a1b`。lightは白 |
| lightの未確定部分 | 既存Radix Slate | 本文・補助文字・hover・選択は既存値を維持 |
| 主操作 | 標準テーマのaccentと `controlPrimary` | dark `#5e6ad2`、light `#626eca`。lightとdark hoverは文字コントラスト4.5:1を保つため明度だけ補正 |
| focus・エラー・警告 | UltraTasksの適合と既存Radix | focusは補助文字色の1px輪郭、エラー・警告はTomato・Amber。Linearの実測色とは呼ばない |

完全な色値、URL、主要資料のSHA-256、合成方法は[design/tokens.json](../design/tokens.json)の `source_evidence` にある。本体CSSの色は注入される `sx` 変数を参照しており、CSS取得だけで通常本体の全テーマ色が判明した扱いにはしない。

インストール済みLinear 1.32.4を起動し、再読み込み後のInbox、All issues、Create issueを確認した。REAの解析shellは1.32.8であり、同じ版とは扱わない。実画面では主要操作の青紫、明るい作成面、コンパクトなheader、状態pillと空状態の配置を確認した。公開bundleのLCHはD50で、元accent・elevated面・変換式・補正量を設計JSONに残している。画面を見る操作と、コードから計算した色を区別する。

## GPUI部品と配布資産

[GPUI Kit](https://github.com/longbridge/gpui-kit) 0.9.0のDialog、DropdownMenu、Popover、Button、Input、Textarea、Selectを既存の入力・保存状態へ接続する。Dialogの表示には `WindowExt::open_dialog` を使う。公開APIの調査対象はcommit `7591a35eced761e3613c01e71ff04c10129dae1a` であり、部品の採用だけでfocus・IME・保存動作の確認済みとはしない。

ローカルの調査記録は `out/ultratasks/linear-parity/implementation.md`、`research/gpui-primitives/public-api.json`、`research/loader/` 配下の取得manifestにある。loaderの取得単位は `public-assets-1lk686hj`、`direct-assets-eee1xhlk`、`create-assets-4v4zecpj` で、それぞれの `manifest.json` にURL・ファイル名・SHA-256を保持する。これらは取得資料の識別用であり、生のCSS・JavaScriptや参照画像をリポジトリへコピーして配布しない。

Interは上流commit `e3a3d4c57d5ecc01453a575621882a384c1995a3` の静的TTFを無変更で使用する。400・500・600・700のファイル、取得アーカイブ、hashは[fonts/source.json](../apps/desktop/assets/fonts/source.json)にある。[SIL OFL 1.1の原文](../apps/desktop/assets/fonts/OFL.txt)と著作権通知を保持する。日本語fallbackのYu Gothic UIは同梱しない。その他の資産・依存の通知は[THIRD_PARTY_NOTICES.md](../THIRD_PARTY_NOTICES.md)を参照する。

## 確認状況

この文書は取得記録とコードの対応を示す。新UIの一覧、作成、詳細、保存、検索、Undo/Redo、時間割、再起動復元は実画面で確認した。対象SHA、確認した幅とテーマ、未解決の支援技術操作は[UIの検証記録](linear-ui-validation.md)に記載する。Linear通常画面との完全な一致やUI完成を判定した記録ではない。

保存・検索・Undo/Redo・仮想化の機能基盤を維持することと、新UIで操作確認を終えたことは別である。CI成功だけで外観の同等性を判断しない。以前の広い参照調査は[ui-reference-study.md](ui-reference-study.md)に残るが、今回の採用値は本書とDESIGN.mdを参照する。
