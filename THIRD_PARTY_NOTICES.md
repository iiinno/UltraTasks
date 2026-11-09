# Third-party assets

実際に取り込んだ素材のみを記録する。正確なcommit SHA、upstreamのファイル、ローカルの配置、変更内容は [`design/upstream-lock.json`](design/upstream-lock.json) に固定する。

| Project | Imported material | License notice |
| --- | --- | --- |
| [Radix Colors](https://github.com/radix-ui/colors) | Slate / Blue / Jade / Amber / Tomatoのlight・darkの12段階の色。値を変えずJSONへ抽出 | [MIT license](third_party/radix-colors/LICENSE) |
| [Lucide](https://github.com/lucide-icons/lucide) | 選定した11個のSVGを無変更でコピー | [Full license](third_party/lucide/LICENSE): ISC、Feather由来部分のMIT通知を含む |

GPUI Kit 0.9.0と、そのGPUI 0.3.11系列をCargo依存として採用した。GPUI KitのソフトウェアはApache-2.0。その他のRust依存の正確な版は`Cargo.lock`、各クレートのライセンスはそのmanifestと同梱通知に記録されている。配布物を公開する際は依存・資産のライセンス通知も同梱する。

React Aria、Radix Themes、Material、shadcn/ui、FullCalendar、ZedのアプリUIは調査先であり、ソースファイルをコピーしていない。これらから実装を移植する場合は、その時点のライセンス・NOTICEを確認し、この台帳に追加する。ZedのUIクレートのGPL条件をGPUI本体のApache条件と混同しない。

## Androidシステムホストの依存

AndroidのTLS検証用AAR、Kotlinランタイム、注釈、D8ビルドツールは、[固定版と配布元](apps/android/dependencies.json)でSHA-256を検証して取得する。著作権・ライセンス通知は[Android依存の通知](apps/android/licenses/NOTICE.md)に保存し、必要な通知をAPKにも同梱する。これらはGPUI画面とは独立したOS・TLS接続用の依存。
