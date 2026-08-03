# Porting a Zola site to grackle

A practical guide, written from an actual port (the *midnight-kitty* recipe
blog on Zola's *serene* theme). It uses plain language and points at the
mechanisms without assuming you've read `DESIGN.md`.

The short version: **most of a Zola site ports in an afternoon.** The content
and config are a mechanical transform. The one real rewrite is the theme,
because grackle has no template engine — but that turns out to be less code
than the Tera templates it replaces.

---

## 1. The mental model

A Zola site is **content + Tera templates + config**. You write Markdown,
Tera templates decide how it looks, and `config.toml` holds settings.

A grackle site is **content + a theme + config-as-queries**:

- **Content** is the same Markdown files, but each becomes a typed *row* in a
  small database. Fields are validated at build time.
- **A theme** is a directory of *data* — HTML fragments with labelled holes,
  plus one stylesheet. No code, no logic.
- **Config** (`grackle.toml`) declares where files land (routes) and what
  lists exist (views/queries), instead of settings a template reads.

The one rule that reshapes everything:

> **A theme has no control flow.** No `{% if %}`, no `{% for %}`.
> A conditional means you're missing a *field*. A loop means you're missing a
> *view*.

Zola leans on Tera for both. In grackle, an "if" becomes a fact the engine
computes (and an empty value just deletes its element), and a "for" becomes a
declared query. Almost every Tera template you have is really one of those two
things wearing template syntax.

---

## 2. The mapping at a glance

| Zola | grackle |
| --- | --- |
| `config.toml` `[extra]` theme toggles | mostly **deleted** — look is theme CSS, not per-site settings |
| site title / base_url / author | `[site]` in `grackle.toml` |
| `content/**/_index.md` (a *section*) | a **tree** page, or a `[routes]` view |
| `content/**/post.md` (a *page*) | a **post** (if dated) or a tree page |
| `+++` TOML / `---` YAML front matter | **both accepted** — but the *keys* must match grackle's schema |
| `[taxonomies] tags` + `tags/*.html` | a `tags` field + one `group_by = "tags"` route |
| Tera templates (`base`, `page`, `section`) | **theme fragments** (`root.html`, `row.html`, a card) |
| `{% extends %}` / `{% block %}` | `root.html` chrome + named **slots** |
| `{% if x %}…{% endif %}` | a schema field (empty value deletes the element) |
| `{% for x %}…{% endfor %}` | a **view** (`[sets]` / `[routes]`) |
| shortcodes (`note`, `figure`, `youtube`) | `[widgets]` (a named HTML wrapper) or `{% image %}` |
| macros (`seo`, etc.) | `[html.head.meta]` — the `<head>` is config |
| `feed.xml` template | a `feed`/`atom` output kind |
| `static/` | an **objects** collection (assets), routed `/{path}` |
| `sass/main.scss` | `theme.scss` (compiled the same way) |
| `get_url` / `permalink` | derived automatically — you never build URLs |

---

## 3. Porting the content and config

### 3.1 `config.toml` → `grackle.toml`

Start almost empty. `extends = "default"` is implied, and the built-in base
config already gives you the three collections (posts, pages, assets), the
`published` set, `/`, `/blog/`, a feed and a sitemap. So a minimal config is
just who the site is:

```toml
root = "."

[site]
url    = "https://example.com"
title  = "My Site"
author = "Me"
```

Then **throw away the Zola `[extra]` block.** Serene alone had ~30 toggles
(blur effect, back-to-top, table-of-contents, comment on/off, nav wrappers…).
In grackle those are decisions the theme's CSS makes, not per-site settings.
A few genuinely-content values (author, a copyright string) survive — see the
theme section for where they go.

### 3.2 Front matter

grackle reads **both** `---` (YAML) and `+++` (TOML) blocks, so you usually
don't need to touch the delimiters. What you *do* need to fix is Zola-specific
**keys**, because every field is schema-checked:

- **Remove `template = "…"`.** grackle picks the layout from the row's shape,
  not from a per-file template name.
- **Flatten taxonomies.** Zola's `[taxonomies] tags = [...]` becomes a plain
  top-level `tags = ["…"]` (or `tags: [...]` in YAML). `tags` is a built-in
  list field.
- **Handle `[extra]`.** Most of it disappears. Anything you actually want to
  keep as data (a `category`, a `cover` image) becomes a real field — declare
  it (next point).
- **Drop Zola-only keys** like `insert_anchor_links`, `weight`, `in_search_index`.

These fields are **built in** and need no declaration: `title`,
`description`, `tags`, `date`, `draft`, `hidden`, `toc`, `noindex`, `cover`,
`image`, plus the engine keys `slot`, `shell`, `theme`.

**Any other field is a build error until you declare it.** Put a
`.schema.toml` in the directory it applies to:

```toml
# recipes/.schema.toml — applies to every file under recipes/
category = { type = "string" }
servings = { type = "int" }
```

Types: `string`, `int`, `bool`, `list`, `image`, `date`, `records`.

### 3.3 The content tree

Zola's *section vs page* distinction goes away. Decide by **dated-ness**:

- **Dated writing → posts.** Files in `_posts/` named
  `YYYY-MM-DD-slug.md` route to `/blog/YYYY/MM/DD/slug/` automatically.
- **Everything else → tree pages.** A file `about.md` lands at `/about/`; a
  folder `recipes/pizza/index.md` lands at `/recipes/pizza/` (and any image
  sitting next to it is picked up as an asset).

So Zola's `content/recipes/pizza/_index.md` becomes
`recipes/pizza/index.md`. The nested `_index.md` convention becomes plain
`index.md`.

### 3.4 Taxonomies and tag pages

This is where grackle *removes* work. In Zola you configure a taxonomy and
write `tags/list.html` and `tags/single.html` to loop over it. In grackle a
tag page is one declared view:

```toml
[routes.tag_index]
paths    = ["/tags/{key}/"]
from     = "published"
group_by = "tags"        # one page per tag, automatically
title    = "Tagged {key}"
```

No template, no loop. `group_by` works on any list-or-scalar field, so
grouping recipes by a `category` field is the same one line.

### 3.5 Assets and styles

- **`static/`** → an **objects** collection. By default images are published
  at a content-addressed `/static/{hash}` address (deduplicated, cache-safe).
  If you want Zola's literal paths (`/img/cat.png`), add one rule:

  ```toml
  [[collections]]
  name = "objects"
    [[collections.rules]]
    match = "**/*.{png,jpg,jpeg,gif,webp,svg}"
    route = "/{path}"
  ```

- **`sass/`** → your theme's `theme.scss` (compiled with the same SCSS
  features). Global `@font-face` and CSS variables move here or into the
  theme's `_tokens.scss`.

- **Markdown image links just work.** A relative `![](pizza.jpg)` next to the
  page resolves and publishes on its own. Raw HTML `<img src="…">` inside
  Markdown resolves too.

- **Internal links** are checked. By default (strict) an internal link must
  name a *source* file (`[About](/about.md)`) or a view (`(view:tag_index)`),
  not a guessed output URL — because grackle derives URLs, so it can catch a
  broken one at build time. If you're mid-port and don't want to convert links
  yet, set `[links] policy = "loose"` to silence it temporarily.

### 3.6 Shortcodes

First, check whether your *content* even uses them — many themes ship
shortcodes the posts never call. For the ones you use:

- Image/figure/embed shortcodes → the built-in `{% image path %}`.
- Simple wrappers (callouts like `note`, `warning`) → a **widget**: a named
  HTML shell with a `{body}` hole, declared in `grackle.toml` as a
  `name = "template"` entry…

  ```toml
  [widgets]
  note = "<aside class='note'>{body}</aside>"
  ```

  …and used in Markdown as a paired tag whose contents fill `{body}`:

  ```markdown
  {% note %}Cold butter is the secret.{% endnote %}
  ```

- Shortcodes that take **arguments** or contain **logic** don't port directly
  — that's the no-control-flow rule again. Rethink them as a field or a view.

---

## 4. Porting the theme

A grackle theme is a small directory. A complete one is often five files:

```
themes/mytheme/
  theme.toml       # usually empty
  _tokens.scss     # every colour / font / size literal
  theme.scss       # the actual styling (@import "tokens")
  root.html        # page chrome: header / main / footer
  row.html         # how one document is arranged
  row--card.html   # how one item in a list looks (optional)
```

Set it in config with `theme = "mytheme"`, and put the directory in your
site's `themes/` folder (the built-in base theme is the fallback, so a theme
only needs to say what's *different*).

### 4.1 Fragments and slots

A fragment is plain HTML with **holes**, marked by `data-slot`:

```html
<!-- root.html -->
<header>
  <a href="/" data-slot="site_title"></a>
  <nav data-slot="nav"></nav>
</header>
<main data-slot="content"></main>
<footer><p data-slot="copyright"></p></footer>
```

Four rules cover everything Tera did with `{% if %}`/`{% for %}`:

1. `data-slot="name"` — the engine fills the element with that part.
2. **An empty part deletes its element.** This replaces every "show this only
   if…" conditional. No tags to show → no `<p class="tags">` at all.
3. `data-fragment="x"` on a slot **repeats** a sub-fragment per item. This
   replaces every `{% for %}` (breadcrumbs, tag pills, list items).
4. `data-slot-href="url"` (or `-src`, `-width`, …) fills an **attribute**; a
   missing value just omits the attribute.

Every slot name is checked at build time against the layout's known parts, so
a typo is an error, not a silently blank page.

The parts a document exposes include: `crumbs`, `title`, `tags`, `hero`,
`intro`, `content`, `pagination`, `relations`. A listing adds `items` (a
stream) and `pagination`. Look at an existing gallery theme's `row.html`
(`themes/almanac/`, `themes/kitty/`) for the full arrangement to copy from.

### 4.2 The site's words: `.slots/`

Your header nav links and footer text are **content**, not theme code, so
they don't live in the theme. Put them in a `.slots/` folder at the site root:

```
.slots/nav.md         a Markdown list of links → fills data-slot="nav"
.slots/copyright.md   → fills data-slot="copyright"
```

```markdown
<!-- .slots/nav.md -->
- [Recipes](/recipes/index.md)
- [About](/about.md)
```

This is why a theme can be shared without carrying one site's menu.

### 4.3 Fonts

**A theme cannot ship font files** — the `themes/` directory isn't served. So:

- Put the preferred font first in the CSS stack and let the system fall back:
  `--font-body: "IBM Plex Sans", system-ui, sans-serif;`. Readers who have the
  font get it; everyone else gets a clean system face.
- If you truly need the webfont, host the `.woff2` files as **site assets**
  (in the content tree, routed `/{path}`) and `@font-face` to that path — but
  now it's the *site's* dependency, not the theme's.

### 4.4 Dark mode

A theme's `<head>` is limited to `<style>` — **no `<script>`**. So a
JavaScript light/dark toggle (which many Zola themes use with `localStorage`)
isn't available in the theme itself. Use the scriptless equivalent:

```scss
:root { --bg: #fff; --fg: #222; }
@media (prefers-color-scheme: dark) {
  :root { --bg: #202124; --fg: #ddd; }
}
```

Same visual result, driven by the reader's OS setting. (A manual toggle would
need the *site* to add the script, not the theme.)

### 4.5 Four gotchas the port will hit

- **The home page shows a "title" and breadcrumbs.** Zola's homepage is
  usually a bare landing. Add `slot: root` to the home file's front matter —
  it skips the document furniture (breadcrumbs, title, hero) and renders just
  the body.
- **The title appears twice.** If the theme renders the title *and* your
  Markdown body starts with `# Title` (a Zola habit, since serene didn't print
  a title), you get it twice. Delete the leading `# Title` from the body.
- **A photo appears twice.** The `hero` field defaults to "the first image in
  the body." If your theme has a hero slot *and* the image is also in the
  body, it shows twice. Either drop the hero slot (if photos live in the body)
  or move the image to a `cover:` field that isn't in the body.
- **A blank broken image.** A page with no hero image still emits an empty
  `<img>`. Hide it in CSS (`.hero img:not([src]) { display:none }`) or don't
  place the hero slot.

---

## 5. What doesn't map cleanly

Be honest with yourself about these before you start:

- **Any Tera template with real logic** — it must become a view (loop) or a
  field (conditional). This is the whole job, not a footnote.
- **Per-page template selection** (`template = "x.html"`) — gone; layout is
  inferred.
- **Per-page conditional `<head>` includes** — e.g. loading KaTeX or Mermaid
  only on pages that set a flag. The computed `<head>` is site-level, so
  per-row conditional assets need a different approach (a widget, or a
  site-wide include).
- **Comment systems** (giscus, Disqus) — no built-in hook; add via the theme
  `<head>` or a widget.
- **Config-driven "about" home widgets** — serene builds an avatar/bio/socials
  block from `[extra]`. grackle has no matching config shape; author that
  block as content in the home file instead.
- **Argument-carrying shortcodes** — the widget system is intentionally
  argument-free.

---

## 6. Verifying the port

```bash
grackle build                 # render everything; build errors are your to-do list
grackle serve                 # live preview, rebuilds on save
grackle explain /some/url/    # everything the engine knows about one page
grackle routes                # the whole URL tree
grackle urls                  # compare the URL set against a reference build
```

`explain` is the one to lean on — it shows a row's fields, its resolved
theme, its shell, and *why* it rendered the way it did. When a page looks
wrong, `explain` usually says why in one screen.

---

## 7. A realistic order of operations

1. Empty `grackle.toml` with `[site]`. Run `grackle build` — see what it says.
2. Convert front matter (delimiters usually fine; fix keys). Declare custom
   fields with `.schema.toml`. Build until it's clean.
3. Move `static/` in; add an objects route if you want literal asset paths.
4. Get it rendering on the **base theme** first — don't theme yet. Confirm the
   content, routes and feed are right.
5. Replace taxonomy templates with `group_by` views.
6. *Then* build the theme: `root.html`, `row.html`, a card, and `theme.scss`
   translated from your old `sass/`. Iterate with `grackle serve`.
7. Fix the four theme gotchas in §4.5 as they appear.

The content is usually right within an hour. The theme is where you'll spend
the afternoon — and it ends up smaller than the Tera it replaced.
