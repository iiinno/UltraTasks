# UI改善で参照した設計と再利用範囲

2026-11-12。今回の対象はWindowsネイティブ版の業務画面。Linearの製品画面を主な参照とし、GPUI Kitの既存部品を使い、RadixとGeistの公開されている設計規約を対応付ける。マーケティングサイトの装飾や、未実装の機能を追加する作業ではない。

## 参照と採用する規則

| 参照 | 確認した内容 | UltraTasksへの適用 |
| --- | --- | --- |
| [Linear 2026年のUI刷新](https://linear.app/now/behind-the-latest-design-refresh) | 控えめなsidebar、小さなicon、非選択文字の減光、暖色寄りのgray、弱い境界線。location barとview barを分ける図 | sidebar・タスク/編集・時間軸の役割を分け、内容を主役にする。通常操作はghost、選択は背景と文字で示す |
| [Linear 2024年のUI再設計](https://linear.app/now/how-we-redesigned-the-linear-ui) | sidebar、tab、header、panelの整列、filter/display用の操作領域 | アイコン列と文字の左端を揃え、検索と絞り込みを対象のpaneへまとめる |
| [Linear Display options](https://linear.app/docs/display-options) | filterと行内に表示するpropertyの区別 | タイトルと状態を優先し、期限・作業時間・所属を補助情報として扱う |
| [Radix spacing](https://www.radix-ui.com/themes/docs/theme/spacing) | 4/8/12/16/24/32/40/48/64px | 既存のspacing tokenを継続し、部品ごとにばらばらな余白を足さない |
| [Radix Button](https://www.radix-ui.com/themes/docs/components/button) | solid/soft/surface/outline/ghost等の階層 | 保存・追加などの主操作と、移動・設定・行内操作の強さを分ける |
| [Radix color scale](https://www.radix-ui.com/colors/docs/palette-composition/understanding-the-scale) | 背景1–2、部品状態3–5、境界6–8、solid9–10、文字11–12 | appのsemantic tokenを通じてdark/lightと状態を揃える |
| [Geist Button](https://vercel.com/geist/button) / [Input](https://vercel.com/geist/input) / [Menu](https://vercel.com/geist/menu) | 主・補助・破壊操作の区別、短いfield label、意味のある入力エラー、Escapeとfocus復帰 | 既存GPUI部品へ対応付け、独自の操作規則を増やさない |

今回用いるsidebar幅、行高、文字サイズ等の具体値は、上記規則を日本語のGPUI画面へ適用する設計値である。Linearの全寸法を実測した値とは呼ばない。

## ローカルのLinear資料

利用者指定の `REA/linear-desktop` は、公式Windows配布物Linear 1.32.8の取得・展開資料。内部packageは `@linear/desktop`、`private: true`、作者はLinear Orbit, Inc.。通常rendererはリモートのWebアプリを読み込み、展開された `out/renderer/index.html` は空だった。通常のsidebar・一覧・編集画面のCSSやスクリーンショットはこの資料に含まれない。

確認できたのは、既定ウィンドウ1280×800、Windows標準titlebar、シェルと背景色の同期、paletteのfocus/Escape、エラー画面の32pxボタン・5px角丸・150msの状態遷移等。エラー画面の値を製品全体のトークンと扱わない。Linear固有コード・画像・フォントは移植していない。

## 実装部品

GPUI Kit 0.9.0 / gpui-pre 0.3.11を維持する。Button、Input、Select、Tooltip、Menu等は公開APIを使い、表層の配置とskinをアプリ側で定義する。既存の仮想化、入力Entity、focus、disabled、validation、保存コマンドを維持する。

KitのSidebarMenuItemには、調査した版でTab・Enter/Space等の不足があるため、sidebarの行にはキーボード操作を備えたButtonを使う。Calendarのキーボード操作にも未確認点があり、この外観変更を理由に既存の日時入力型を全面置換しない。非公開のManagedTooltipExt等は使わない。

## 出典とライセンス

- GPUI Kit/Base/Component/AssetsとGPUIはApache-2.0。既存の通知と原文を保持する。Kitの固定元commitは `7591a35eced761e3613c01e71ff04c10129dae1a`。
- Radix [Themes](https://github.com/radix-ui/themes/blob/main/LICENSE)、[Primitives](https://github.com/radix-ui/primitives/blob/main/LICENSE)、[Colors](https://github.com/radix-ui/colors/blob/main/LICENSE)はMIT。既存のRadix色の通知を保持する。今回、React部品のコードをそのままGPUIへコピーしていない。
- [Lucide](https://github.com/lucide-icons/lucide/blob/main/LICENSE)はISC、一部Feather由来iconはMIT。両方の該当通知を保持する。
- Linearの画面構成とGeistの公開部品規約は参照資料。公開ページであることを、それぞれの独自コード・ブランド資産・fontの再配布許諾とは扱わない。[Linear brand](https://linear.app/brand)、[Vercel brands](https://vercel.com/geist/brands)も参照。
- Linearの公式比較画像はワークスペースの `out/ultratasks/ui-redesign/research/linear-official-images-vm685v65/` に検討用として保存した。配布するアプリの資産には含めない。画像取得元は同ディレクトリの `sources.json`。

## 確認の基準

1280/900幅、dark/light、日本語の長いタイトル、空・入力エラー・取得失敗、選択・hover・focus・disabledを実画面で確認する。本文やplaceholderのコントラストは4.5:1、必要な非文字の状態表示は3:1を設計基準とする。Radixの特定条件下のAPCA保証を、任意の合成色のWCAG適合と混同しない。

見た目の比較と、入力→時間割当→完了→Undo→再起動の動作確認を分けて記録する。UIの変更だけで、未測定の描画速度や120Hz達成を主張しない。
