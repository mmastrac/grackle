---
title: Ledger
layout: page
toc: true
tags: [gallery]
date: 2026-07-20
---
One warm column, a serif face, and a palette that flips to dark without a
single rule knowing about it. This is the theme to copy when the site is
mostly prose and you want to stop thinking about the chrome.

## What to look at

- **[Notes](view:ledger_notes_index)** — the listing face: summaries,
  truncation, tags, pagination two to a page.
- **[Guide](guide/index.md)** — a section tree, a page outline, crumbs, and an
  index-less directory.
- **[Shelf](view:ledger_shelf_index)** — the card face, with a featured
  item.
- **[Wall](view:ledger_wall)** — the gallery face: masonry over objects.

## A typography specimen

Body copy, with `inline code`, a [link back to the gallery](/index.md), some
**bold** and some *emphasis*. The measure here is whatever `--measure` says it
is, and the leading is `--leading`.

> A blockquote, which every theme in the gallery draws with `--line-strong`
> and nothing else.

### A third-level heading

1. An ordered item.
2. Another, to show the list rhythm.

```rust
// A fenced block, so --font-mono and the --bg-sunken panel get an outing.
fn main() {
    println!("the geometry is in the CSS");
}
```

| token | what it moves |
| --- | --- |
| `--scale` | the entire heading ladder |
| `--measure` | the reading column |
| `--accent` | links, and whatever chrome the theme paints with them |
