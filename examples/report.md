# mdpdf 第三季開發報告

本報告整理 **mdpdf** 專案在第三季的進度、技術決策與下一步規劃。
mdpdf 是一個以 Rust 撰寫的命令列工具，能把 Markdown 報告直接轉成 PDF，
不需要 pandoc、瀏覽器或 LaTeX。

## 一、進度摘要

本季完成三個里程碑：

1. **M0 骨架**：記憶體中的 Typst World、內嵌 Noto Sans TC 字型
2. **M1 區塊解析**：標題、段落、清單、程式碼區塊、引言
3. **M2 行內解析**：強調、連結、圖片、autolink、字元參照
   - CommonMark 規格測試通過率 *100%*（652/652）
   - 行內程式碼如 `mdparse::parse()` 也能正常顯示

目前仍待完成的項目：

- GFM 擴充：表格、刪除線、任務清單、腳註
- LaTeX 數學公式
- 字型與樣式的命令列選項

## 二、架構

整體流程分為四個階段，每個階段都可以獨立測試：

![各階段程式碼行數](images/chart.svg)

> **設計原則**：parser 與 Typst 完全解耦，AST 是唯一的介面。
> 所有文字都以字串字面值輸出，Markdown 內容永遠不會被當成 Typst 標記解讀。

### 2.1 解析器

解析器沒有任何外部相依，採用規格建議的兩階段演算法。
下面是公開 API 的用法：

```rust
use mdparse::{parse, BlockKind};

fn count_headings(markdown: &str) -> usize {
    // 只計算最上層的標題
    parse(markdown)
        .blocks
        .iter()
        .filter(|b| matches!(b.kind, BlockKind::Heading { .. }))
        .count()
}
```

### 2.2 效能

在 22 頁的測試文件上，完整轉換（解析、排版、輸出 PDF）約需 **110 毫秒**[^bench]，
遠低於 1 秒的目標。若同時掃描系統字型，時間會增加到約 360 毫秒。

| 項目 | 規格範例數 | 通過 | 通過率 |
| :--- | ---: | ---: | :---: |
| CommonMark 0.31.2 | 652 | 652 | 100% |
| GFM 擴充 | 23 | 23 | 100% |
| 開啟擴充後的 CommonMark | 652 | 652 | 100% |

### 2.3 數學公式

公式以 LaTeX 撰寫，由 MiTeX 轉成 Typst 排版。行內公式如 $e^{i\pi} + 1 = 0$，
區塊公式：

$$
\hat{f}(\xi) = \int_{-\infty}^{\infty} f(x)\, e^{-2\pi i x \xi}\, dx
$$

$$
\mathbf{A} = \begin{bmatrix} a_{11} & a_{12} \\ a_{21} & a_{22} \end{bmatrix},\quad
\operatorname{sgn}(x) = \begin{cases} 1 & x > 0 \\ 0 & x = 0 \\ -1 & x < 0 \end{cases}
$$

### 2.4 待辦事項

- [x] 表格、刪除線、任務清單、腳註
- [x] CJK 寬鬆強調：**「重點」**這樣的寫法也能正確加粗
- [x] LaTeX 數學公式（M4）
- [ ] ~~自己畫 PDF~~ 改用內嵌 Typst 排版
- [ ] 字型與樣式選項（M5）

[^bench]: 以 release 版在 EndeavourOS 上量測，不含系統字型掃描。

<!-- pagebreak -->

## 三、參考資料

- 規格：[CommonMark 0.31.2][spec]
- 排版引擎：[Typst](https://typst.app "Typst 官網")
- 問題回報：<https://github.com/example/mdpdf/issues>
- 聯絡信箱：<dev@example.com>

特殊字元測試：&copy; 2026、`Vec<String>`、Option<T>、#set、\$x\$、@ref 都會原樣輸出。

[spec]: https://spec.commonmark.org/0.31.2/
