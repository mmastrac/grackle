---
title: Miroir
layout: page
toc: true
tags: [gallery]
date: 2026-07-20
---
A fixed sidebar rail and a card feed, after
[ojeda-e.com](https://ojeda-e.com/) and Zola's
[daisy](https://www.getzola.org/themes/daisy/). Daisy ships 37 colour
schemes; here that is four subtheme tokens over one palette — try
`miroir:rose`, `miroir:forest` or `miroir:slate`.

## What to look at

- **[Notes](view:miroir_notes_index)** — the listing face: summaries,
  truncation, tags, pagination two to a page.
- **[Guide](guide/index.md)** — a section tree, a page outline, crumbs, and an
  index-less directory.
- **[Shelf](view:miroir_shelf_index)** — the card face, with a featured
  item.
- **[Wall](view:miroir_wall)** — the gallery face: masonry over objects.

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
