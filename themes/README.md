# The theme gallery

Six themes. Each is a complete grackle theme (§5e) — fragments plus a
stylesheet, no code — and each is now *only its differences*, because the base
they all sit on is compiled into the engine.

| theme | files | shape | the claim it tests |
| --- | --- | --- | --- |
| [`vanilla`](vanilla/) | 1 | the user-agent stylesheet, 2026 edition | a theme can be one file and still be a theme |
| [`ledger`](ledger/) | 5 | one warm column, serif, dark mode | the baseline: what a prose site should start from |
| [`marginalia`](marginalia/) | 5 | text + margin column, Tufte-ish | *all* geometry is theme CSS — even sidenotes and margin crumbs |
| [`terminal`](terminal/) | 5 | monospace, dark-first, nothing rounded | a total repaint is a token block, not a rewrite |
| [`atlas`](atlas/) | 8 | sticky section tree, cards, gallery | one theme covers docs, a card index and a photo wall |
| [`miroir`](miroir/) | 8 | fixed sidebar rail, card feed, accent chrome | a strongly art-directed look is still tokens + geometry |

`miroir` is drawn from two real sites — [ojeda-e.com](https://ojeda-e.com/) for
the fixed dark rail, the right-aligned nav and the centred title over a hairline
rule; Zola's [daisy](https://www.getzola.org/themes/daisy/) for the saturated
brand bar, the raised card feed with icon meta rows, and the accent call to
action. Daisy advertises 37 colour schemes; here that is one palette plus three
subtheme tokens (`miroir:rose`, `miroir:forest`, `miroir:slate`), which is the
same feature expressed as the thing this gallery argues for.

## The base is in the binary

`crates/grackle/assets/base/` — fourteen fragments and four stylesheets,
embedded with `include_str!` exactly as `parts.toml` is, and inherited by every
theme. A theme's fragment replaces the base's of the same name; every kind it
declines keeps the base arrangement. **A site with no `themes/` directory at
all renders through it**, which is what turns "the null theme" from merely
complete into actually good — try `examples/minimal`, which has no theme and
comes out as semantic HTML with a stylesheet.

Two consequences worth knowing before you write a seventh theme:

- **The base is structure, never decoration.** A rule belongs there if a theme
  would have to re-derive it (the measure, a nav that is a row not a bulleted
  list, indentation under a nested list, the reset, the search overlay). It does
  *not* belong there if a theme would have to **undo** it — a `content:`
  separator, an ellipsis, a pill. Those live in `vanilla/`, and the line got
  drawn the hard way: an ellipsis and a comma in the base turned up inside
  ledger's tag pills and on top of its truncation fade.
- **Ship a shell, own the frame.** The base's page geometry keys on
  `[data-frame]`, an attribute its own `shell.html` stamps. Write your own
  shell and you inherit none of it — which is correct, because your header may
  be a sticky full-bleed bar (atlas) or a fixed sidebar rail (miroir), and a
  centred `--measure` column would be actively wrong for both.

## Looking at them

```bash
grackle --config grackle/theme-preview/grackle.toml serve --port 8083
```

[`theme-preview/`](../theme-preview/) is a small site whose content tree is
structurally identical subtrees, one per theme, with a rule cascading `theme:`
over each. `/ledger/notes/` and `/miroir/notes/` are the *same* rows in the
*same* shapes, so you compare themes by opening two tabs. Each theme has a
landing (typography specimen, page outline), a paginated listing, a section tree
with an index-less directory, a card shelf and a masonry wall. There is a
`grackle-themes` entry in `.claude/launch.json` for the same thing.

## Using one

```bash
cp -r grackle/themes/terminal themes/terminal
```

That is the whole install — the base comes with the binary, so a copied theme
has no companion directory to remember. The engine loads every directory under
`themes/` (skipping `_`-prefixed ones) and compiles each `theme.scss` to
`/css/<name>.css`, except `default`, which keeps `/css/main.css`. Theme is
chosen per row (§5a): a row opts in with front matter, or a rule cascades it to
a subtree:

```toml
[[collections.rules]]
match = "recipes/**"
defaults = { theme = "terminal" }
```

Rename the directory to `default` and it is the site-wide theme. Subtheme tokens
ride after a colon — `theme: "ledger:dark"` renders through `ledger` with
`data-subtheme="dark"` on `<html>`, which CSS subselects via
`[data-subtheme~="dark"]`. Every theme here uses that to force light or dark
against `prefers-color-scheme`; `marginalia` adds a second, independent token
(`wide`) to show they compose: `theme: "marginalia:dark:wide"`.

## The token contract

```
_tokens.scss   every literal the theme owns. THE FILE YOU EDIT.
theme.scss     the geometry. Reads tokens; holds no literals.
*.html         only the fragments this theme rearranges.
```

**The smallest theme is one file.** A directory holding nothing but
`_tokens.scss` is a complete theme: it retunes the palette, the type ratio and
the measure, and inherits every rule. That is the rung to start on.

> ### What you keep, and the one thing you ask for
>
> The reset and the **type ladder** — heading sizes, weight, block rhythm —
> apply to every theme, always. Writing a `theme.scss` never takes them away.
> They are safe to impose because they read only tokens: change `--size` and
> `--scale` and the whole hierarchy moves, without restating a rule.
>
> The **skins** — the blockquote rule, the code panel, table borders, the
> callout — are opt-in:
>
> ```scss
> @import "tokens";
> @import "skin";
> ```
>
> The boundary is measured, not chosen. Applied under grack.com's theme (which
> has a complete type sheet of its own) the ladder is *inert* — its reset wins
> every conflict and the ladder only fills gaps — while the skins move a
> paragraph 19px and a listing page 61px, because a theme with opinions about
> a blockquote will fight one it did not ask for.
>
> `vanilla` imports neither, and is still a whole page: an unskinned blockquote
> is the browser's, which is the point of that theme.

The reset, the element defaults, the search overlay and the whole token
vocabulary come from the base, bound to CSS **system colours** and `ui-*`
platform faces — so a theme that overrides nothing still has a complete,
accessible, dark-mode-aware value set underneath it. Overriding is the normal
case, not a requirement.

The two arrive as declared cascade layers, `@layer base, theme` (§5e's stated
order), which is what makes a theme's rule win over the base's regardless of
which selector is more specific.

These are the names every theme may assume. Take a block from one theme, paste
it into another, and it works.

**Palette** — `--bg`, `--bg-sunken`, `--bg-raised`, `--fg`, `--fg-muted`,
`--fg-faint`, `--accent`, `--accent-hover`, `--on-accent`, `--line`,
`--line-strong`, `--selection`, `--print-bg`, `--print-fg`

**Type** — `--font-body`, `--font-head`, `--font-ui`, `--font-mono`,
`--root-size`, `--size`, `--scale`, `--leading`, `--leading-tight`,
`--tracking`, `--tracking-caps`, `--weight-body`, `--weight-head`, and the
derived ladder `--size-xs` … `--size-3xl` — chained `calc()`s of `--size` and
`--scale`, so changing the ratio moves the whole hierarchy together.

**Space** — `--space` and `--space-xs`/`-sm`/`-md`/`-lg`/`-xl`, `calc()`
multiples of the one base value.

**Geometry** — `--measure`, `--margin-col`, `--gutter`, `--pad-x`, `--radius`,
`--radius-lg`, `--border`, `--rule`, `--rule-strong`, `--shadow`, `--shadow-lg`.
`--rule` is a whole border shorthand (`var(--border) solid var(--line)`), which
is why no rule below a token file ever names a colour to draw a line.

**Links and motion** — `--underline`, `--underline-offset`, `--transition`

**Components** — `--date-col`, `--nav-gap`, `--pill-pad`, `--header-pad`,
`--hero-width`, `--overlay-top`, `--overlay-max-h`. Themes add their own
(`--prompt`/`--marker` in `terminal`, the `--card-*` family in `atlas` and
`miroir`, `--rail-*`/`--cta-*` in `miroir`) and document them in place.

### The one thing that cannot be a token

A media query's condition resolves before custom properties do, so breakpoints
are Sass variables (`$collapse` in `marginalia`, `$drop-aside`/`$drop-sidebar`
in `atlas`, `$unpin-rail` in `miroir`), declared at the foot of `_tokens.scss`
so they are still edited in one place. `ledger` and `terminal` have no
breakpoint at all — their single column is already fluid.

## Themes are partial

Every theme here is. `vanilla` ships no fragments and three rules. `ledger`,
`marginalia` and `terminal` ship four files each — the shell (they want a search
button the base does not assume), the document, the summary, and their tokens.
Everything else — crumbs, tags, neighbours, relations, outline entries,
pagination, page links, link lists, the figure variant — is inherited, and each
theme styles that markup through its `data-kind` rather than through a class it
invented.

`atlas` and `miroir` add `listing--cards`, `listing--gallery` and
`summary--card` on top: a row picks its face with `variant = "cards"`, the face
is a fragment plus CSS, and nothing in the engine knows what a card is. A theme
with no such fragment silently renders the plain listing instead — which the
preview site shows side by side on purpose.

## Notes worth reading before writing a seventh

- **Rule 2 deletes an empty part's element, not a wrapper the fragment
  invented.** `atlas` gets its optional sidebars free because the rails *are*
  parts; `terminal` and `miroir` group two parts in a `.doc-meta` bar and pay
  for it with `:not(:has(*)) { display: none }`. The base pays the same tax for
  its own `<footer>`.
- **A flat fragment plus CSS Grid means one row per child.** `marginalia`'s
  margin column started as `grid-template-columns` and was wrong: every part
  auto-places into its own row, so four margin items grow four empty rows
  opposite them and the prose starts *below* its own marginalia. Floats pulled
  out of a padding inset express "beside"; grid expresses "table".
- **A placeholder link is a conditional.** `<a>` with no `href` is how the
  engine says "current page" or "nowhere to go", so the inert tail crumb, the
  current page number and an index-less tree node are all `a:not([href])` in the
  base reset. No fragment branches on any of them.
- **`aria-current` and `data-relation` come from the engine.** Style them; don't
  reinvent them. The language switcher in every theme is the `translations`
  relation repositioned by CSS — it keeps its place in reading order and in the
  accessibility tree, and only its pixels move.
- **Flags are attributes, not content.** `truncated` on a summary and `tree` on
  a document arrive as `data-truncated` / `data-tree` on the fragment root.
  `ledger`'s read-more fade is a rule on a fact.
