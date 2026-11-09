# Third-party assets

実際に取り込んだ素材のみを記録する。正確なcommit SHA、upstreamのファイル、ローカルの配置、変更内容は [`design/upstream-lock.json`](design/upstream-lock.json) に固定する。

| Project | Imported material | License notice |
| --- | --- | --- |
| [Radix Colors](https://github.com/radix-ui/colors) | Slate / Blue / Jade / Amber / Tomatoのlight・darkの12段階の色。値を変えずJSONへ抽出 | [MIT license](third_party/radix-colors/LICENSE) |
| [Lucide](https://github.com/lucide-icons/lucide) | 選定した11個のSVGを無変更でコピー | [Full license](third_party/lucide/LICENSE): ISC、Feather由来部分のMIT通知を含む |

GPUI Kit、React Aria、Radix Themes、Material、shadcn/ui、FullCalendar、Zedは現段階では調査先。コードはまだ取り込んでいない。これらから実装を移植する場合は、その時点のライセンス・NOTICEを確認し、この台帳に追加する。
