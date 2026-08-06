# Themes: distribution, inheritance, and the floor

This file is self-contained: everything it assumes from `../DESIGN.md` (§5a,
§5b, §5e) is restated in §0. Pending work lives in `../TODO-1.0.md` (theme
ladder, chrome parts); this file is the spec those checkboxes point at.

The pain being designed away: in most SSGs a theme is a submodule, a fork, or
a hand-copied directory, and the moment you touch one file you can never
update again. The design goal is a **ladder of customization** (§2) where
every rung is strictly more effort and strictly more power, no rung requires
understanding the rung above it, and — the part other SSGs miss — there is a
mechanical path back down (`theme derive`, §4).

## 0. Platform facts

- **Themes are partial**: any kind a theme declines to arrange falls through to
  canonical (generic semantic markup: `data-kind`, `data-slot`, flags as
  `data-<fact>` attributes). Already an inheritance mechanism with one parent.
- **The chrome file is `root.html`**, binding the part kind `root`. It may be
  a bare fragment, which is the body chrome and what every theme here writes;
  or document-shaped, with a `<head>` **fenced to `<style>`** and a `<body>`;
  or head-only, inheriting the base's chrome. The engine writes `<html>` and
  computes the head in every case — so writing your own `<html>` or doctype
  is a load error rather than a wrapper the engine unwraps for you, and so is
  prose left beside the `<head>` and `<body>`. A fenced `<style>` **leaves
  through the CSS**: compiled as SCSS into the theme layer of the theme's
  sheet, after `theme.scss`. No page carries an inline `<style>`; every page
  carries one stylesheet link.
- **Variant misses degrade**: a row asking for `listing--cards` without that
  fragment falls back to `listing`, then canonical. Row variants are requests.
- **Subthemes**: `theme: "ledger:dark:wide"` renders through `ledger` with
  `data-subtheme="dark wide"` on `<html>`; CSS subselects via
  `[data-subtheme~="…"]`.
- **Identity slots** come from `.slots/` files, not theme files.
- **Token contract**: shared `--bg`, `--size`, `--space`, etc.; all gallery
  themes written entirely in `var(--…)`.
- **`theme.toml`** is the per-theme config §3 specs — `extends` and
  `[subthemes]`. The `[subthemes]` half is **built**: tokens validate at
  load and the scheme pair feeds §10's control. `extends` / `contract` are
  refused as unbuilt until the chain lands.
- **Chrome parts** (§10): capability widgets — `axes` (built), `search`,
  `feed`, `scheme`, `profile_notice` — fill on the root part map from
  declared facts and delete by the empty-part rule when the fact is absent.

## 1. The two guarantees

1. **Any theme on any site**: a theme cannot name site-specific things
   (unknown slots are load errors; identity slots come from the tree), and
   rule 2 (empty part deletes its element) makes absent data silent.
2. **Any site under any theme**: variants degrade (§0), unarranged kinds fall
   to a floor that looks intentional (§7), and knob customization (§2 rung 1)
   survives theme switches because token names are a cross-theme contract.

Both directions get a checker: `grackle theme check` (§4) validates a theme's
fragments and CSS against the engine schemas standalone, and lints token
usage against the contract vocabulary.

## 2. The ladder of customization

| rung | you want | you do | theme files touched |
|---|---|---|---|
| 0 | a look | `cp -r` a gallery theme, then `[site] theme = "name"` (§8, built); `grackle theme add <url>` is still specced | none |
| 1 | different colors/fonts/spacing | site-owned root `.style.scss`: `:root { --accent: … }` — the overlay layer sits above theme CSS | none |
| 2 | a variant the theme ships | `theme: ledger:dark` subtheme token | none |
| 3 | different arrangement | derived theme: `extends` + shadowed fragments and/or `_tokens.scss` (§3) | none (parent pristine) |
| 4 | full control | edit the installed theme; the lockfile knows (§4) | all |

The independence rule: rung *n* must not require understanding rung *n+1*.
Rung 1 is deliberately **not** a theme mechanism at all — it needs no new
machinery, and because the token vocabulary is a contract, a rung-1 override
survives not just theme updates but theme *switches*. Documentation should
state that as a guarantee; `theme check`'s token lint is what keeps it true.
Each rung below 1 that is still unbuilt has a checkbox in `TODO-1.0.md`.

## 3. Inheritance: `extends`

`theme.toml` — the `[subthemes]` half built, the `extends` half still
spec — carries:

```toml
name        = "mytheme"          # optional; directory name is identity
extends     = "ledger"           # a theme directory name
# extends = { name = "ledger", git = "https://…" }   # installable parent
contract    = 1                  # part-schema major version (§4)

[subthemes]                      # the token vocabulary, declared (below)
dark  = { scheme = "dark" }
light = { scheme = "light" }
wide  = { }
```

**Fragments: shadow, by file name.** Effective fragment set = union down the
chain, child wins. Implemented at load, not render: `Themes::load_all`
resolves chains (cycles and unknown parents are load errors naming the
chain), then builds each theme's `Fragments` as the merged map. Everything
downstream — render fallback, identity-slot derivation, variant resolution —
already works on a `Fragments` value and needs no change. The root fragment
resolves the same way: nearest in chain; identity slots derive from the
merged result. Its two halves are independent — a chain member may shadow
the chrome, the head `<style>`, or both, because `split_root` runs per
theme before the merge.

**CSS: concatenation, tokens cascade by the platform's own rule.** No Sass
load-path tricks — Sass resolves imports file-relative, so partial shadowing
across directories would lie. Instead:

```
css(theme) = css(parent)                                  # empty for a root
           + compile(theme.scss)                          # if present
           | compile(_tokens.scss as :root block)         # else, if present
           + compile(root.html's head <style>)            # if present
```

The last line is built (IO.md I5); the chain above it is not. **A member's
own CSS is two files in a fixed order** — the general sheet, then what
`root.html` says about the theme's own frame — and that order is the same
reason the chain is ordered child-last: the more specific statement of intent
comes later. Both halves compile through grass with the theme directory on
the load path, so a head `<style>` may nest and may `@import "tokens";` like
any other file the theme writes.

Custom properties are last-wins, so a child `_tokens.scss` listing only the
two vars it changes is *complete*, stays correct when the parent adds tokens,
and never copies anything. A child with real rules ships a small `theme.scss`
that `@import "tokens";`-s its own file and states its rules; the parent's
CSS is already above it in the output. Output files are per-concrete-theme
(`/css/<name>.css`, `default` keeps `/css/main.css`), unchanged.

The minimal derived theme is therefore two files:

```
themes/mine/theme.toml       extends = "ledger"
themes/mine/_tokens.scss     :root { --accent: #b5651d; --measure: 65ch; }
```

Depth is unbounded, practically 1–2. Subtheme tokens compose unchanged (CSS
from any chain member may select `[data-subtheme~="…"]`).

> **Concatenate into NESTED LAYERS, not into one.** Plain concatenation puts
> parent and child rules in the same cascade level, which recreates exactly
> the specificity war `@layer base, theme` was introduced to settle one level
> up: a parent's `.card .title` beats a child's `.title` no matter that the
> child is later and more specific about wanting to win. The landed base/theme
> split proves the shape works; extend it down the chain —
>
> ```css
> @layer reset, base, theme, overlay, post;
> @layer theme.root, theme.mid, theme.leaf;   /* chain order, root first */
> ```
>
> — so a child always outranks its parent by layer, and `revert-layer` walks
> the chain one step at a time. Sub-layers are ordered by their declaration,
> so the emitter states the chain once and each member's CSS goes in
> `@layer theme.<member>`. Worth building this way from the first commit: the
> failure it prevents is silent and only shows up in someone else's theme.
>
> **A member's CSS is one sub-layer, both halves of it** (IO.md §6's
> multi-theme scoping paragraph, written at I5). `theme.scss` and the root
> head `<style>` are ordered against each other by source position *inside*
> `theme.<member>`, not by layers of their own — so a child shadows its
> parent by layer and states its own two files in order, and the two
> questions never interfere. That is what makes §0's "the root's two halves
> shadow independently" safe: shadowing is by file name, ranking is by layer,
> and neither is doing the other's job.
>
> **The same construct points sideways.** With several themes live (the theme
> axis), one artifact holds many themes' rules, and `@layer theme.<name>` in
> chunk order settles precedence between *themes* exactly as it settles it
> between chain members. Layers order rules but do not stop them matching, so
> the merged case also wants the stamped root attribute (`data-theme`, beside
> `data-subtheme`) as the scope. Both are emitter-side and inert while the
> sheets stay chunked per theme — IO.md §6 has the argument.

### `[subthemes]`: declared tokens, declared schemes

Two consumers, one table. A subtheme token is unvalidated today —
`theme: ledger:drak` stamps `data-subtheme="drak"` and names nothing the
engine knows — so the table is first the **token vocabulary**: a spec token
no chain member declares is a load error naming the knowns. Second, a token
may carry **scheme semantics**: `scheme = "dark" | "light"` says forcing
this token forces that color scheme. A theme whose declared subthemes cover
both schemes is what the `scheme` chrome part (§10) reads as "this theme
can switch" — and the five two-scheme gallery themes already ship the CSS
this declares (`:root[data-subtheme~="dark"]` and its light twin over a
`prefers-color-scheme` default), so the declaration is catching up to the
stylesheets, not asking for new ones. Resolution follows fragments: union
down the chain, child wins per token name.

### Back-tested edges

Three consequences of the union model, found by walking inheritance
scenarios against the design. None changes the model — the union makes each
*legible* rather than wrong — but all three are the difference between
inheritance that works on day one and inheritance that survives a parent
update two years later. Each gets a test in §10 step 1 and a lint in
`theme check`:

1. **Mixed-lineage variants.** The union is per-file, so a child that
   shadows `listing.html` but not `listing--cards.html` renders cards
   through the parent's variant on the child's base styling. Correct —
   shadowing is by name, variants are names — but the least obvious merge
   consequence: a child restyling a kind heavily should shadow its variants
   too. `theme check` warns when a chain splits a `kind`/`kind--variant`
   pair across themes.
2. **Identity-slot drift.** Identity slots derive from the merged root's
   slot set. A child root that *drops* a slot the parent had (`copyright`,
   say) silently disconnects the site's `.slots/copyright.md` — a file the
   site author owns goes dark with no signal. Correct behavior, wrong
   silence: `theme check` (and `theme list`'s chain view) names identity
   slots the chain lost relative to a parent.
3. **Token deletions, not additions.** Additions are the case the cascade
   solves. The asymmetry is a parent update *removing* a token: a child's
   two-line `_tokens.scss` still overrides fine, but a child rule reading
   the vanished var fails silently at computed-value time — CSS has no
   unknown-var error. This is why the token lint runs against the *resolved
   chain*, never the leaf theme alone.

The two-file derived theme (rung 3, tokens-only) has no edges left and is
considered de-risked; fragment shadowing is de-risked modulo edge 1.

## 4. Distribution: the `theme` subcommand

**Vendor, never submodule.** The site is a database that lives in git;
installed themes are files in `themes/<name>/`, committed like everything
else. Provenance lives in `themes/.lock.toml` — installer metadata is the
site's, not the theme author's (a fork must not carry a stale origin):

```toml
[themes.paperback]
git       = "https://github.com/…/paperback"
ref       = "v2"                        # requested (branch/tag); absent = default branch
commit    = "8c1f2…"                    # resolved at install
[themes.paperback.files]                # content hashes at install time
"theme.scss"   = "sha256:…"
"summary.html" = "sha256:…"
```

| command | does |
|---|---|
| `theme add <url>[@ref] [--name n]` | shallow-fetch to cache, copy tree in, write lock. Follows `extends = { git = … }` recursively; refuses on `contract` mismatch with a message naming both versions |
| `theme update [name]` | fetch; if every local file hash matches the lock, replace wholesale and re-lock. Else refuse, listing exactly the edited files, and point at `derive` |
| `theme list` | installed themes, chain, origin, clean/dirty per lock |
| `theme new <name> [--extends x]` | scaffold rung 3: `theme.toml` + commented empty `_tokens.scss` listing the contract names |
| `theme derive <name> [new]` | move the *edited* files (per lock hashes) into a new theme extending it, restore pristine parent files, re-lock, print the front-matter/rule change to re-point rows |
| `theme check [name]` | validate fragments + CSS selectors against engine schemas without a site; lint `--…` names against the token contract, always on the resolved chain (§3 back-tested edges: split variant pairs, lost identity slots, vars no ancestor defines) |
| `theme try <url>` | install to cache only (loaded last, never committed); preview via the dev override (§5); `add` keeps it |

`derive` is the load-bearing command: because inheritance is file shadowing,
"the files you edited" *already are* a valid derived theme — the command is
nearly `mv` plus two lines of TOML. It converts the classic SSG failure mode
(hacked vendor theme, updates now scary) into rung 3 mechanically, which is
what makes rung 4 safe to allow.

Update strategy is refuse-and-derive only. Three-way merging vendored files
is git's job; the lock carries the base commit for anyone who wants it.

## 5. Dev mode

- **`?theme=name[:tokens]` query override** in the dev server, gated to the
  dev profile: render any page through any loaded theme, front matter
  untouched. This is both the experimentation loop and the standing test of
  guarantee 2 — flip through every theme on any page.
- Theme edits already invalidate by `Template(...)` key; the new requirement
  is that a child theme's pages invalidate on *ancestor* edits (the chain is
  known at load — invalidate by every chain member's key).
- `theme try` caches load last so it can shadow nothing by accident.

## 6. The vanilla principle

`vanilla/` (the "2026 user-agent theme") demonstrates the design principle:
**delegate every aesthetic decision to the platform; keep only the decisions
it refuses to make.** Palette = CSS system colors; dark mode = `color-scheme:
light dark`; type = `system-ui` / `ui-monospace`. The engine base fragments
fuse labels and links (semantic element choices); CSS carries only
platform-delegated visuals. Two working rules: **a rule belongs in the base if
a theme would have to re-derive it; it does not if a theme would have to undo
it.** **Ship a root, own the frame** — a theme's own `root.html` inherits none
of the base's page geometry, because a sticky header or sidebar would need to
undo a centred measure.

## 7. The floor: base as a compiled-in theme

The base is a directory of fragments and stylesheets, compiled into the binary
and merged under every theme. **Every site renders reasonably with no theme at
all** — the guarantee strengthens from "reasonable with every theme" to
"reasonable with nothing."

**Cascade layers**: `@layer reset, base, theme, overlay, post`. Base and theme
are declared; `overlay` (site-owned `.style.scss`) and `post` (per-post CSS)
remain planned. A theme rule beats the base's regardless of selector specificity
(no arms race). The review test: **a rule belongs in the base if a theme would
have to re-derive it; it does not if a theme would have to undo it.** This is
backed by measurement, not taste.

**Three tiers, and the boundary between them is measured.** The reset and the
type **ladder** — heading scale, weight, block rhythm — are unconditional. Only
the **skins** are opt-in (`@import "skin";`): the blockquote rule, the code
panel, table borders, the callout.

The measurement is the answer to "could the base just always apply?" — it can,
for half of itself. Under a theme with a complete type sheet of its own the
ladder is **inert**: the theme's reset zeroes everything the ladder sets, so the
theme wins every conflict and the ladder only fills gaps. The skins are not
inert — on a listing they move a paragraph 19px and the page 61px, because a
blockquote gains a left border and a code block gains a panel. What makes the
ladder safe to impose is that it reads *only tokens*: a theme retunes the entire
hierarchy through `--size`/`--scale` without restating a rule, which is a
stronger sense of "overridable" than the cascade alone provides.

Shipping a `theme.scss` is the only thing that declines the skins. A
tokens-only theme, and a site with no theme at all, get them automatically —
they have nobody else to ask. So writing your first `theme.scss` costs you the
code panel and the blockquote rule, whose absence is legible, rather than the
heading hierarchy, whose absence would be alarming.

### Authoring rules

- **Breakpoints are Sass variables, not custom properties** — a media
  query's condition resolves before custom-property substitution.
- **Rule 2 deletes an empty part's element, not a wrapper the fragment
  invented** — a fragment that groups parts in its own wrapper pays for the
  emptiness check itself.
- **A flat fragment plus CSS Grid can create empty rows** — floats out of a
  padding inset express "beside"; grid expresses "table".
- **A placeholder link is a conditional**: `<a>` with no `href` is how the
  engine says "current page" or "nowhere to go" — style `a:not([href])`,
  don't reinvent the check.
- **`aria-current` and `data-relation` are engine-stamped, never
  authored** — style them, don't reinvent them.
- **Flags are attributes, not content** — `data-truncated`, `data-tree` on
  the fragment root, not a template conditional.

## 8. Configuration

`[site] theme = "name[:tokens]"` in `grackle.toml` roots the per-row cascade
(front matter → rule defaults → site default); the `default` directory name
is honored as a fallback so existing sites don't move.
`Themes::resolve(row_spec)` fills the key in the one place all five render
paths already pass through: a row that names nothing renders as if it
named the site's theme.

Three consequences: the site default is a full spec, so its tokens apply
(`theme = "ledger:dark"` is a one-line site-wide dark mode) — unless a row
names its own theme, whose tokens do not inherit the site's; a listing
wears it too, unless `unanimous_theme` (§5h) already resolved one from its
members; and `default` is spellable with no `themes/` directory, since
`default` *is* the base theme (§7) — any other unknown name is a load error.

One honest edge: `/css/main.css`, the `default` theme's sheet, still emits
even when `[site] theme` names another theme, referenced by nothing unless
a row says `theme: default`. The real fix — emitting only the sheets a
build actually referenced — is a pass this design does not invent.

## 10. Chrome parts: a widget is a fact's chrome

Fill rules and `[subthemes]` are built; the rest is tracked in
`TODO-1.0.md` ("Chrome parts"). The pull model replaced hand-pasted
per-theme search buttons that drifted per root.

**A widget is a fact's chrome. You install the fact; the chrome follows.**
There is no registration step, because the declaration that creates the fact
is the registration — the law `[shells]`, `[axes]` and `[markers]` already
follow: a registry declares vocabulary, a declaration spends it, and
*spending* is what activates.

### The parts and their facts

Chrome parts fill on the root part map, one per capability:

| part | fills iff | carries |
|---|---|---|
| `axes` | an axis this row spends has ≥2 members | member list *(built)* |
| `search` | a materialized route wears `shell = "search"` | `@search` label, loader URL |
| `feed` | a materialized route wears `shell = "atom"` | route URL, `@feed` label |
| `scheme` | the resolved theme declares both schemes (§3) and no rung of the theme cascade forced one | state + labels |
| `profile_notice` | the active profile is not `default` (serve only) | profile name |

No fact → empty part → the empty-part rule deletes the element. That is the
entire gating mechanism, and it is why every root may carry every chrome
slot unconditionally. Facts come from three places — config (a route), the
corpus (axis members), the theme (a declared capability) — and the part
algebra does not care which. Two routes wearing the same fold shell: the
first-declared wins the chrome part, with a warning naming both.

### Stand-down: a declared choice removes the offered one

`default_content`'s law, reused. A site or row whose theme spec wears a
scheme token (`ledger:dark`) has decided, so the `scheme` part empties on
every page that stamp reaches; delete the search route and the button is
gone site-wide. **Disabling is upstream, at the fact** — never a
presentation flag beside it — so config stays the single source of truth
and `explain` never has to see through a half-state.

### Primitives: a theme styles three things, not N widgets

Default fragments are built from a closed set of chrome primitives, stamped
as structure: `data-chrome="button"`, `"dropdown"` (the `<details>` shape
`axes` already uses), `"expando"` (icon that becomes a field, via
`:focus-within`; its CSS is deferred until a fragment uses it). A
`_chrome.scss` engine partial ships the structural floor on the reset tier
and the decorated look on the skin tier — the `_type.scss`/`_skin.scss`
split, one row over. A theme that styles the three primitives has styled
every widget, including the ones that do not exist yet. Primitives are to
chrome what tokens are to color: the contract that makes a widget look
native under a theme that never heard of it.

### The cluster: `chrome.html`

The base root places one cluster slot whose inline default is the ordered
widget set:

```html
<div data-slot="chrome" data-fragment="chrome">
	<div data-slot="search" data-fragment="search_button"></div>
	<div data-slot="scheme" data-fragment="scheme_button"></div>
	<nav data-slot="axes" data-fragment="axis" aria-label="Other versions"></nav>
</div>
```

The inline body IS `chrome.html` (THEME.md's inline-default rule,
unchanged); shipping the file shadows it. The name is `chrome`, not
`widgets`, on purpose: `[widgets]` in config already means markdown body
expansions, and one word does not get two engine meanings. The base root
and every gallery root place the cluster; the wrapper is `display:
contents` in the base sheet, so a header's flex or grid sees the same
children whether a root places the cluster or the slots individually.

**Forward compatibility is the point, not tidiness.** A theme that places
the cluster opted into the *category*: when the engine grows a widget, it
lands in the cluster's default body and appears in every such theme with
zero edits. The alternative — individual slots per widget — re-runs the
search-button failure on every future widget.

Reordering has a ladder:

- **Theme-level**: ship `chrome.html` in your order, or split a widget out
  by placing its slot individually. One law covers the collision —
  **first writer per part** (the precedence law's existing clause): an
  individually-placed slot wins and the cluster's copy of that part
  empties. Nothing renders twice.
- **Site-level, and positional**: `.slots/chrome.html`
  shadows the `chrome` fragment across every loaded theme, beating a
  theme's own `chrome.html` — the tree-overlay rung of the precedence law
  applied to a fragment-bearing slot. It reorders, drops, or **mints** —
  literal author markup is legal beside the engine holes, so a site's own
  dropdown can sit inside the widget row with no theme touched. And it is
  **positional like every other fill**: `docs/.slots/chrome.html` answers
  for its subtree, nearest wins up the source path, and the root's file is
  the degenerate one-directory case. Listings and landings resolve from the
  site root, exactly as their nav fill does. The two spellings that would
  silently not apply are load errors: a markdown flavor, and a locale
  suffix (the holes fill with localized parts already).

`feed` sits outside the cluster in the base root's footer — the shipped
demonstration that splitting out is ordinary. The skip-to-content link is
**not** a widget: it has no fact and is always correct, so it is hardcoded
in the base root like the frame itself.

### Derived surfaces

- **Head**: the expand form gains a second pool —
  `{ from = "shell.atom", rel = '"alternate"', type =
  '"application/atom+xml"', title = 'site.title', href = 'site.url + url' }`
  expands over fold routes wearing that shell, every member in route order
  (discoverability lists all feeds, where the chrome part links only the
  first). An explicit `rel` frees the key to be a name, because `alternate`
  is already spent on hreflang. An absent pool source stands the entry down —
  the base inherits its feed line everywhere — and `require = true` upgrades
  absence to a refusal: at load for an undeclared axis, at build for a shell
  no route wears. The fold vocabulary is closed, so `shell.<typo>` is a load
  error rather than an empty pool.
- **`search.js` fetches the index at the path the search route declared** —
  substituted at emission, never matched by hand. This deletes the
  `SEARCH_VER`-coupling class of 404 (both example sites ship it today) and
  the loader URL rides a fragment attribute so `baseurl` holds.
- **The scheme boot script**: ~150 bytes, engine-emitted inline in the
  computed head only when the `scheme` part fills, applying the stored
  preference (`localStorage["grackle:scheme"]`) before first paint. It
  touches only scheme-family tokens, so `wide` survives. Auto = both tokens
  removed; the theme's `prefers-color-scheme` default rules. The head fence
  is untouched — it governs themes, and this is the engine's head, like the
  stylesheet link.

### Checks

- **Capability without slot** — a search route exists and the resolved
  theme places neither a `search` slot nor the cluster: load warning naming
  the theme and the slot. (The identity-slot-drift lint of §3, one slot
  family over.)
- **Slot without capability** — free by construction: empty deletes.
- `theme check` runs both on the resolved chain when it lands.

### Honest edges, named now

- **localStorage unavailable** (privacy modes): the control still cycles
  for the session and falls back to auto on reload. Silent and harmless.
- **The theme axis**: the stored scheme preference is site-global while
  token names (`dark`/`light`) are conventional across themes — so dark
  survives crossing from `/ledger/…` to `/miroir/…`. The right behavior,
  and it falls out for free; a sentence of doc, not a mechanism.
- **kitty, recipes and almanac declare no schemes** and get no control —
  honest. Their upgrade is a dark palette plus the two forcing rules plus
  two lines of `theme.toml`, all optional.
- **`light-dark()`** could retire the gallery's `@mixin palette-dark` +
  `@media` double spelling (the base's system colors are already the
  degenerate case). An optional simplification, not a prerequisite.
