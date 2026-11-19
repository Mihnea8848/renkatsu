# News

`news.toml` holds the posts that Renkatsu Studio shows on its home page,
under the banner. The program reads the file from the `main` branch, so a
post is live once it is merged. Releases show up in the same list by
themselves; they need no post.

## Write a post

Add a `[[post]]` at the top of `news.toml`:

```toml
[[post]]
date = 2026-11-18
title = "Starter Content is available"
summary = "You can use Renkatsu without Koikatsu now."
url = "https://github.com/Mihnea8848/renkatsu/releases"
```

Only `date` and `title` are needed. The other keys:

| Key | What it does |
|---|---|
| `summary` | One or two lines under the title. |
| `url` | The page a click opens. It must be on `https://github.com/Mihnea8848/`; any other link is dropped and the post shows without one. |
| `title_ja`, `summary_ja` | The Japanese version. English shows where they are missing. |
| `since` | The post shows only from this version of Renkatsu on, for example `"0.2.0"`. |
| `until` | The last day the post shows, written like `date`. |

## Rules

- Plain text only. Markdown and HTML are shown as written.
- Titles are cut at 80 characters and summaries at 200.
- The file must stay under 64 KB, or the program ignores it.
- The page shows the eight newest entries, posts and releases together.
- Keys the program does not know are ignored, so older versions keep
  working when the format grows.

The program asks for the file at most every six hours and sends nothing
about the user. The news can be turned off in Preferences.
