---
title: Anatomy of a table of contents
tags: [writing]
toc: true
---
A table of contents is a claim about structure: that the headings below
form a tree, and that the tree is worth navigating. This note is long
enough, and structured enough, to test that claim.

## Where outlines come from

Headings nest by level. An `h2` owns the `h3`s beneath it, the way a
directory owns its files — hierarchy derived from position, read in depth.

### The id problem

Every heading needs an anchor, and the anchor must match what the renderer
emitted. Extract the outline from the same parse that emits the ids and the
two cannot drift apart.

### The depth problem

Nobody wants an outline of `h5`s. Depth limits are production policy — the
levels you hide must never be shipped.

## Where outlines go

Into a slot, like everything else. A theme places the outline in a margin,
below the title, or nowhere at all; the markup does not change.

## What this note is for

Being outlined. If you can read a nested table of contents for this note
somewhere on this page, that feature shipped.
