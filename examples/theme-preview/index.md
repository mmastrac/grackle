---
title: Theme gallery
---
Eight themes, **one small blog, and one declaration per page type**. The
themes are an axis: every post, guide page and shelf entry below is a
single row published at a route per look, and each landing is a single
*view* materialized across the axis. This page is not on it.

This page is the index of the looks rather than one of them, so its rule does
not spend the axis and it publishes once, at `/`, wearing the base theme
rather than any of the eight — which is how a row opts out.

| | blog | shelf | wall |
|---|---|---|---|
| vanilla | [›](view:blog_index?theme=vanilla) | [›](view:shelf_index?theme=vanilla) | [›](view:wall_index?theme=vanilla) |
| ledger | [›](view:blog_index?theme=ledger) | [›](view:shelf_index?theme=ledger) | [›](view:wall_index?theme=ledger) |
| marginalia | [›](view:blog_index?theme=marginalia) | [›](view:shelf_index?theme=marginalia) | [›](view:wall_index?theme=marginalia) |
| terminal | [›](view:blog_index?theme=terminal) | [›](view:shelf_index?theme=terminal) | [›](view:wall_index?theme=terminal) |
| atlas | [›](view:blog_index?theme=atlas) | [›](view:shelf_index?theme=atlas) | [›](view:wall_index?theme=atlas) |
| miroir | [›](view:blog_index?theme=miroir) | [›](view:shelf_index?theme=miroir) | [›](view:wall_index?theme=miroir) |
| almanac | [›](view:blog_index?theme=almanac) | [›](view:shelf_index?theme=almanac) | [›](view:wall_index?theme=almanac) |
| recipes | [›](view:blog_index?theme=recipes) | [›](view:shelf_index?theme=recipes) | [›](view:wall_index?theme=recipes) |

## Subthemes

A subtheme is `theme: "ledger:dark"` — the same theme with `data-subtheme`
stamped on `<html>`, repainting from the token file alone. They ride the same
axis, so each is just another look with its own route and its own place in
the picker.

| | blog | what it changes |
|---|---|---|
| ledger:dark | [›](view:blog_index?theme=ledger:dark) | a dark palette swap |
| marginalia:wide | [›](view:blog_index?theme=marginalia:wide) | a wider measure |
| miroir:rose | [›](view:blog_index?theme=miroir:rose) | a rose accent |
| miroir:forest | [›](view:blog_index?theme=miroir:forest) | a green accent |
| miroir:slate | [›](view:blog_index?theme=miroir:slate) | a slate accent |
| recipes:spicy | [›](view:blog_index?theme=recipes:spicy) | a hotter accent, and a 🌶 |
