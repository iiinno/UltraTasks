# Design foundation

共通素材の初期基盤。画面・Rustコードへの組み込みは未実施。

| ファイル | 意味 |
| --- | --- |
| `palettes.json` | Radix Colorsから実値を抽出したlight / darkの5パレット |
| `tokens.json` | UltraTasks用の用途別の配色割り当てと、余白・文字・密度・動きの初期案 |
| `icons/` | Lucideから無変更で取得したSVG |
| `upstream-lock.json` | コピーした素材のcommit・ファイル・変更内容 |
| `palette-preview.svg` | トークンを解決して生成した比較用シート |
| `contrast-report.json` | 不透明sRGB色の22組の通常文字コントラスト。ネイティブ実機や全面的なアクセシビリティ適合の検証ではない |
| `concepts/` | 画像生成による画面の方向性とレビュー項目 |

色はロックしたRadixソースから取り込んでおり、MIT通知を保持する。その他のトークンはUltraTasksの初期選定値。余白スケール・文字サイズの検討には[Radix Themes](https://github.com/radix-ui/themes/tree/main/packages/radix-ui-themes/src/styles/tokens)、短いdurationと曲線の検討には[Material Web motion tokens](https://github.com/material-components/material-web/blob/main/tokens/versions/v0_192/_md-sys-motion.scss)を参照した。これらのCSS / Sassソース自体はコピーしていない。日本語の可読性、文字拡大、動きを減らす設定、タップ領域、GPUIの座標系は今後実機で調整する。

ライトテーマの状態色は、薄い背景にstep 11を重ねると今回の組み合わせでは4.5:1を下回ったため、文字をstep 12へ割り当てた。ブランド色の既定値をそのまま全用途に適用せず、実際の組み合わせで確認する。
