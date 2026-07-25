---
title: Tokens
layout: page
order: 1
toc: true
---
Everything a theme in this gallery can be told is a custom property.

## Palette

Twelve names, from `--bg` to `--selection`. Dark mode overrides exactly this block and nothing else.

## Type

`--size` and `--scale` are the only two numbers; the ladder from `--size-xs` to `--size-3xl` is chained `calc()` off them, so changing the ratio moves the whole hierarchy together.

## Space

One `--space`, five multiples of it.

## Geometry

`--measure` is the reading column. `--rule` is a whole border shorthand, which is why no rule outside the token file names a colour to draw a line.

## A line too wide for a phone

The base reset scrolls a `pre` itself, so a long line never scrolls the PAGE — check this one at 375px in every theme, `vanilla` included, since vanilla imports no typography at all.

```
$ curl -fsSL https://example.com/some/quite/long/path/that/keeps/going | sh -c 'echo far wider than any phone viewport'
```
