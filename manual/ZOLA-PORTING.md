# Porting a Zola site to grackle

Content and config are a mechanical transform. The theme is most of the rewrite.

Note: unlike Zola, Grackle has no template engine but the result is usually
smaller than the Tera it replaces.

---

## 1. Mental model

A Zola site is content, Tera templates, and `config.toml`.

A grackle site is content, a theme, and config that declares routes and queries:

- Content is the same Markdown. Each file becomes a typed row. Fields are
  checked at build time.
- A theme is _HTML fragments_ with labelled holes, plus a stylesheet. No code.
  No logic.
- Config (`grackle.toml`) declares where files land and what lists exist.
  Templates do not read settings.

The rule:

> A theme has no control flow. No `{% if %}`. No `{% for %}`.

Zola puts looping and conditionals in Tera. In Grackle, a conditional is a field
that feed into a filter. A loop is similarily a view.

> A conditional is a missing field. A loop is a missing view.

---

## 2. Mapping

| Zola                                       | grackle                                           |
| ------------------------------------------ | ------------------------------------------------- |
| `config.toml` `[extra]` theme toggles      | deleted — look lives in theme CSS                 |
| site title / base_url / author             | `[site]` in `grackle.toml`                        |
| `content/**/_index.md` (section)           | a tree page, or a `[routes]` view                 |
| `content/**/post.md` (page)                | a post (if dated) or a tree page                  |
| `+++` TOML / `---` YAML front matter       | both accepted; keys must match the schema         |
| `[taxonomies] tags` + `tags/*.html`        | a `tags` field + one `group_by = "tags"` route    |
| Tera templates (`base`, `page`, `section`) | theme fragments (`root.html`, `row.html`, a card) |
| `{% extends %}` / `{% block %}`            | `root.html` chrome + named slots                  |
| `{% if x %}…{% endif %}`                   | a schema field (empty deletes the element)        |
| `{% for x %}…{% endfor %}`                 | a view (`[sets]` / `[routes]`)                    |
| shortcodes                                 | `[widgets]` or `{% image %}`                      |
| macros (`seo`, etc.)                       | `[html.head.meta]`                                |
| `feed.xml` template                        | a `feed` / `atom` output kind                     |
| `static/`                                  | an objects collection, routed `/{path}`           |
| `sass/main.scss`                           | `theme.scss`                                      |
| `get_url` / `permalink`                    | derived automatically                             |

---

## 3. Content and config

### 3.1 `config.toml` → `grackle.toml`

Start empty. `extends = "default"` is implied. The base config already gives you
posts, pages, assets, the `published` set, `/`, `/blog/`, a feed, and a sitemap.
A minimal config is identity:

```toml
root = "."

[site]
url    = "https://example.com"
title  = "My Site"
author = "Me"
```

Delete the Zola `[extra]` block. Serene alone had ~30 toggles (blur,
back-to-top, table-of-contents, comments, nav wrappers). Those are theme CSS
decisions, not per-site settings. A few content values (author, copyright)
survive — see the theme section.

### 3.2 Front matter

grackle reads both `---` (YAML) and `+++` (TOML). You usually keep the
delimiters. Fix Zola-specific keys — every field is schema-checked:

- Remove `template = "…"`. Layout comes from the row's shape, not a per-file
  template name.
- Flatten taxonomies. `[taxonomies] tags = [...]` becomes top-level
  `tags = ["…"]` (or `tags: [...]` in YAML). `tags` is built in.
- Handle `[extra]`. Most of it goes away. Values you keep (`category`, `cover`)
  become real fields — declare them below.
- Drop Zola-only keys: `insert_anchor_links`, `weight`, `in_search_index`.

Built-in fields (no declaration): `title`, `description`, `tags`, `date`,
`draft`, `hidden`, `toc`, `noindex`, `cover`, `image`, plus engine keys `slot`,
`shell`, `theme`.

Any other field is a build error until you declare it. Put a `.schema.toml` in
the directory it applies to:

```toml
# recipes/.schema.toml — applies to every file under recipes/
category = { type = "string" }
servings = { type = "int" }
```

Types: `string`, `int`, `bool`, `list`, `image`, `date`, `records`.

### 3.3 Content tree

Zola's section vs page distinction goes away. Use dated-ness:

- Dated writing → posts. Files in `_posts/` named `YYYY-MM-DD-slug.md` route to
  `/blog/YYYY/MM/DD/slug/`.
- Everything else → tree pages. `about.md` → `/about/`. `recipes/pizza/index.md`
  → `/recipes/pizza/`. Sibling images become assets.

Zola's `content/recipes/pizza/_index.md` becomes `recipes/pizza/index.md`.
Nested `_index.md` becomes plain `index.md`.

### 3.4 Taxonomies and tag pages

In Zola you configure a taxonomy and write `tags/list.html` and
`tags/single.html`. In grackle a tag page is one view:

```toml
[routes.tag_index]
paths    = ["/tags/{key}/"]
from     = "published"
group_by = "tags"        # one page per tag
title    = "Tagged {key}"
```

No template. No loop. `group_by` works on any list or scalar field, so grouping
recipes by `category` is the same line.

### 3.5 Assets and styles

- **`static/`** → an objects collection. By default images publish at a
  content-addressed `/static/{hash}` URL. For Zola's literal paths
  (`/img/cat.png`):

  ```toml
  [[collections]]
  name = "objects"
    [[collections.rules]]
    match = "**/*.{png,jpg,jpeg,gif,webp,svg}"
    route = "/{path}"
  ```

- **`sass/`** → the theme's `theme.scss`. Global `@font-face` and CSS variables
  go here or in `_tokens.scss`.

- Relative Markdown images (`![](pizza.jpg)`) resolve and publish on their own.
  Raw HTML `<img src="…">` in Markdown resolves too.

- Internal links are _always_ checked. By default an internal link must name a
  source file (`[About](/about.md)`) or a view (`(view:tag_index)`).

### 3.6 Shortcodes

Check whether your content uses them — themes ship lots of shortcodes that are
not even used. Used ones become _widgets_ in `grackle.toml`:

- Image / figure / embed → built-in `{% image path %}`, or a widget.
- A wrapper with a body (callout) — a template with a `{body}` hole:

  ```toml
  [widgets]
  note = "<aside class='note'>{body}</aside>"
  ```
  ```markdown
  {% note %}Cold butter is the secret.{% endnote %}
  ```

- Arguments fill `{name}` holes from `key="value"` pairs. A template with no
  `{body}` is self-closing:

  ```toml
  [widgets]
  figure  = "<figure><img src=\"{src}\"><figcaption>{body}</figcaption></figure>"
  youtube = "<iframe src=\"https://youtube.com/embed/{id}\"></iframe>"
  ```
  ```markdown
  {% figure src="/cat.png" %}A cat{% endfigure %} {% youtube id="dQw4w9WgXcQ" %}
  ```

  A quoted value is a literal. A bare value is an expression over the row:
  `{% byline who=title %}` fills `{who}` from the row's `title`.

- Head assets. A widget can pull a `<script>` / `<style>` / `<link>` into the
  `<head>` of pages that use it (deduped):

  ```toml
  [widgets.math]
  template = "<span class='math'>{body}</span>"
  head     = "<link rel=stylesheet href='…katex.css'><script defer src='…katex.js'></script>"
  ```

  KaTeX, Mermaid, or a comment embed (giscus) load only on pages that use them.

- `$$…$$` display math desugars to the `math` widget. Define a `math` widget as
  above and `$$E=mc^2$$` keeps working. A `$$` with no `math` widget is a build
  error.

Control flow inside a shortcode (Tera `{% for %}` / `{% if %}` in its body) does
not port. Reshape it as a view or a field.

---

## 4. Theme

A grackle theme is a small directory. A complete one is often five files:

```
themes/mytheme/
  theme.toml       # usually empty
  _tokens.scss     # colours / fonts / sizes
  theme.scss       # styling (@import "tokens")
  root.html        # page chrome: header / main / footer
  row.html         # how one document is arranged
  row--card.html   # how one list item looks (optional)
```

Set `theme = "mytheme"` and put the directory in the site's `themes/` folder.
The built-in base theme is the fallback — a theme only needs to say what's
different.

### 4.1 Fragments and slots

A fragment is HTML with holes marked by `data-slot`:

```html
<!-- root.html -->
<header>
  <a href="/" data-slot="site_title"></a>
  <nav data-slot="nav"></nav>
</header>
<main data-slot="content"></main>
<footer><p data-slot="copyright"></p></footer>
```

Four rules cover what Tera did with `{% if %}` / `{% for %}`:

1. `data-slot="name"` — fill the element with that part.
2. An empty part deletes its element. This replaces "show only if…".
3. `data-fragment="x"` on a slot repeats a sub-fragment per item. This replaces
   `{% for %}` (breadcrumbs, tag pills, list items).
4. `data-slot-href="url"` (or `-src`, `-width`, …) fills an attribute. A missing
   value omits the attribute.

Slot names are checked at build time. A typo is an error, not a blank page.

Document parts include: `crumbs`, `title`, `tags`, `hero`, `intro`, `content`,
`pagination`, `relations`. A listing adds `items` and `pagination`. Copy
arrangement from an existing gallery theme's `row.html` (`themes/almanac/`,
`themes/kitty/`).

### 4.2 Site words: `.slots/`

Header nav and footer text are content, not theme code. Put them in `.slots/` at
the site root:

```
.slots/nav.md         Markdown list of links → data-slot="nav"
.slots/copyright.md   → data-slot="copyright"
```

```markdown
<!-- .slots/nav.md -->

- [Recipes](/recipes/index.md)
- [About](/about.md)
```

A shared theme does not carry one site's menu.

### 4.3 Fonts

A theme cannot ship font files — `themes/` is not served.

- Put the preferred font first in the CSS stack and fall back:
  `--font-body: "IBM Plex Sans", system-ui, sans-serif;`.
- To ship a webfont, host `.woff2` files as site assets (content tree, routed
  `/{path}`) and `@font-face` to that path. That dependency belongs to the site,
  not the theme.

### 4.4 Dark mode

A theme's `<head>` may contain `<style>` only — no `<script>`. A JavaScript
light/dark toggle with `localStorage` is not available in the theme. Use
`prefers-color-scheme`:

```scss
:root {
  --bg: #fff;
  --fg: #222;
}
@media (prefers-color-scheme: dark) {
  :root {
    --bg: #202124;
    --fg: #ddd;
  }
}
```

A manual toggle needs a script from the site, not the theme.

### 4.5 Gotchas

- **Home page shows a title and breadcrumbs.** Zola's homepage is often a bare
  landing. Add `slot: root` to the home file's front matter — it skips
  breadcrumbs, title, and hero, and renders the body only.
- **Title appears twice.** If the theme prints the title and the Markdown starts
  with `# Title` (common with serene), delete the leading heading.
- **Photo appears twice.** `hero` defaults to the first image in the body. If
  the theme has a hero slot and the image is also in the body, it shows twice.
  Drop the hero slot, or move the image to a `cover:` field.
- **Blank hero on imageless pages.** An `<img>` with no source, and its empty
  wrapper, collapse to nothing. To keep an empty frame, mark it
  `data-no-collapse`.

---

## 5. What does not map cleanly

- Tera templates with real logic must become a view (loop) or a field
  (conditional). That is the work.
- Per-page template selection (`template = "x.html"`) is gone. Layout is
  inferred.
- Config-driven "about" home widgets — serene builds an avatar / bio / socials
  block from `[extra]`. Author that block as content in the home file instead.

Two things that used to be hard now port via widget head fragments (§3.6):

- Per-page conditional `<head>` includes (KaTeX, Mermaid) — a widget with a
  `head` fragment. `$$…$$` desugars to `math`.
- Comment systems (giscus, Disqus) — a widget whose `head` injects the embed
  script, used on the pages that want comments.

---

## 6. Verifying

```bash
grackle build                 # render; build errors are the to-do list
grackle serve                 # live preview, rebuilds on save
grackle explain /some/url/    # everything the engine knows about one page
grackle routes                # the URL tree
grackle urls                  # compare the URL set against a reference build
```

`explain` shows a row's fields, theme, shell, and why it rendered the way it
did. When a page looks wrong, start there.

---

## 7. Order of operations

1. Empty `grackle.toml` with `[site]`. Run `grackle build`.
2. Convert front matter (delimiters usually fine; fix keys). Declare custom
   fields with `.schema.toml`. Build until clean.
3. Move `static/` in. Add an objects route if you want literal asset paths.
4. Render on the base theme first. Confirm content, routes, and feed.
5. Replace taxonomy templates with `group_by` views.
6. Build the theme: `root.html`, `row.html`, a card, and `theme.scss` from the
   old `sass/`. Iterate with `grackle serve`.
7. Fix the §4.5 gotchas as they appear.

Content is usually right within an hour. The theme takes the afternoon.
