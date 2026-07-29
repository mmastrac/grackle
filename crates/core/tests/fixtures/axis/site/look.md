---
title: Look
---
# Heading

An axis member is named with a `?axis=value` selector, which is the only way
to link one: a link resolves to a ROW, and a row answers with its canonical
URL. The selector reads as a query string and resolves to a PATH, because a
member's address is derived like every other URL here.

- [this page, canonical](look.md?theme=default)
- [this page, loud](look.md?theme=loud)
- [the other page's light tier](tiers.md?serialization=light_html)

A value the axis does not declare is a load error naming the ones it does, and
a selector naming an axis that does not cover the row is an error too. Any
other query string — `?utm=x` — stays the literal suffix it always was.
