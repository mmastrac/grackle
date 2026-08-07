# Grackle: The ultimate static site generator

Grackle is currently a WIP.

Grackle is the static site generator that powers [grack.com](https://grack.com).

## Getting Started

Install the latest git version directly:

```sh
cargo install --git https://github.com/mmastrac/grackle/ grackle
```

## Configuration

Grackle is configured using a `grackle.toml` file. See examples in the `examples` directory.

An empty `grackle.toml` file will get you started. Run `grackle serve` to start a development server, drop some posts in the `_posts` folder.

Manual coming soon.

## How it works

Your site's files form a database. Grackle turns those rows into a site when you define routes. A file gets published to a row (or rolled up in a view with other files).

You can define schema for rows at the site-level, or futher down the tree. The schema ends up filling slots in your theme, and that makes a page.

Grackle is built on modern web technologies and works best with modern CSS features.

It supports more advanced features as well:

 - Need multilingual support? Add a locale axis and a file/directory pattern to identify translations. (example config coming soon)
 - Need to tweak a theme? Override just part of the base theme. (example config coming soon)
 - Want search? Add a search index and most themes will provide a search UI. (example config coming soon)
 - Have to hide some content from search? Drop an empty `.hidden` file in a path to quickly set that flag (example config coming soon)
