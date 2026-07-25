# The theme gallery

Six themes. Each is a complete grackle theme — fragments plus a stylesheet, no
code — and each is *only its differences* over the base, now compiled into the
engine.

| theme | files | shape | the claim it tests |
| --- | --- | --- | --- |
| [`vanilla`](vanilla/) | 1 | the user-agent stylesheet, 2026 edition | a theme can be one file and still be a theme |
| [`ledger`](ledger/) | 5 | one warm column, serif, dark mode | the baseline: what a prose site should start from |
| [`marginalia`](marginalia/) | 5 | text + margin column, Tufte-ish | *all* geometry is theme CSS — even sidenotes and margin crumbs |
| [`terminal`](terminal/) | 5 | monospace, dark-first, nothing rounded | a total repaint is a token block, not a rewrite |
| [`atlas`](atlas/) | 8 | sticky section tree, cards, gallery | one theme covers docs, a card index and a photo wall |
| [`miroir`](miroir/) | 8 | fixed sidebar rail, card feed, accent chrome | a strongly art-directed look is still tokens + geometry |

## View them

```bash
grackle --config grackle/theme-preview/grackle.toml serve --port 8083
```

The [`theme-preview/`](../theme-preview/) site renders identical content under each theme.
Compare themes by opening two tabs: `/ledger/notes/` and `/miroir/notes/` are
the same rows in the same shapes. Entry: `grackle-themes` in `.claude/launch.json`.

## Install one

```bash
cp -r grackle/themes/terminal themes/terminal
```

Then set it in `grackle.toml`:

```toml
[site]
theme = "terminal"
```

The base comes with the binary, so a copied theme needs no companion directory.

## Token contract reference

**Palette** — `--bg`, `--bg-sunken`, `--bg-raised`, `--fg`, `--fg-muted`,
`--fg-faint`, `--accent`, `--accent-hover`, `--on-accent`, `--line`,
`--line-strong`, `--selection`, `--print-bg`, `--print-fg`

**Type** — `--font-body`, `--font-head`, `--font-ui`, `--font-mono`,
`--root-size`, `--size`, `--scale`, `--leading`, `--leading-tight`,
`--tracking`, `--tracking-caps`, `--weight-body`, `--weight-head`

**Space** — `--space`, `--space-xs`, `--space-sm`, `--space-md`, `--space-lg`, `--space-xl`

**Geometry** — `--measure`, `--margin-col`, `--gutter`, `--pad-x`, `--radius`,
`--radius-lg`, `--border`, `--rule`, `--rule-strong`, `--shadow`, `--shadow-lg`

**Links and motion** — `--underline`, `--underline-offset`, `--transition`

**Components** — `--date-col`, `--nav-gap`, `--pill-pad`, `--header-pad`,
`--hero-width`, `--overlay-top`, `--overlay-max-h`

These names are a cross-theme contract: take a block from one theme, paste it
into another, and it works.

