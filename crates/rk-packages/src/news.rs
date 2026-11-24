// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: LicenseRef-PolyForm-Shield-1.0.0

//! The news of the project selector's home page: the posts of
//! `news/news.toml` in the repository and the releases on GitHub, merged
//! by date. The releases also say whether a newer Renkatsu is out, so one
//! request serves both. Nothing about the user is sent; the answers are
//! kept in the cache folder and asked for again after six hours.

use crate::manifest::version_key;
use crate::update::Release;
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::io::Read;
use std::path::{Path, PathBuf};

/// The posts, as the repository's main branch has them.
pub const POSTS: &str = "https://raw.githubusercontent.com/Mihnea8848/renkatsu/main/news/news.toml";
/// The latest releases of the repository.
pub const RELEASES: &str = "https://api.github.com/repos/Mihnea8848/renkatsu/releases?per_page=10";
/// A news file larger than this is refused.
pub const MAX_BYTES: usize = 64 * 1024;
/// How many pieces of news the page shows at most.
pub const SHOWN: usize = 8;
/// Seconds an answer stays good before it is asked for again. GitHub
/// answers an unchanged list with a short "not modified" thanks to the
/// ETag, so asking often costs little, and a new release shows soon.
pub const FRESH: u64 = 15 * 60;
/// What a link must start with to be one: the project's own GitHub.
const LINKS: &str = "https://github.com/Mihnea8848/";
const TITLE_CHARS: usize = 80;
const SUMMARY_CHARS: usize = 200;
const AUTHOR_CHARS: usize = 40;
const BODY_CHARS: usize = 16000;
/// What starts a paragraph of a post's body that is a heading.
pub const HEADING: &str = "## ";
/// Where the news file's pictures are: a post names one by its path
/// below the news folder, `images/posing.png`.
pub const PICTURES: &str = "https://raw.githubusercontent.com/Mihnea8848/renkatsu/main/news/";
/// GitHub's store of what is pasted into a release's notes, the other
/// place a picture may come from.
const ATTACHMENTS: &str = "https://github.com/user-attachments/assets/";
/// A picture larger than this is not shown.
pub const PICTURE_BYTES: u64 = 8 << 20;

/// A picture of a post: a paragraph of its own, `![caption](address)`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Picture {
    pub url: String,
    pub caption: String,
}

/// The picture a paragraph is, when it is one and nothing else.
pub fn picture(paragraph: &str) -> Option<Picture> {
    let rest = paragraph.trim().strip_prefix("![")?;
    let (caption, rest) = rest.split_once("](")?;
    let url = rest.strip_suffix(')')?;
    if url.is_empty() || url.contains(char::is_whitespace) {
        return None;
    }
    Some(Picture {
        url: url.to_owned(),
        caption: caption.trim().to_owned(),
    })
}

/// The full address of a picture the page may show: one of the news
/// folder, by its path there or in full, or one pasted into a release.
fn picture_url(src: &str) -> Option<String> {
    let src = src.trim();
    let clean = |path: &str| {
        !path.is_empty()
            && !path.contains(char::is_whitespace)
            && path
                .split('/')
                .all(|part| !part.is_empty() && part != "." && part != "..")
    };
    if let Some(path) = src.strip_prefix(PICTURES) {
        return clean(path).then(|| src.to_owned());
    }
    if let Some(id) = src.strip_prefix(ATTACHMENTS) {
        return clean(id).then(|| src.to_owned());
    }
    (!src.contains(':') && clean(src)).then(|| format!("{PICTURES}{src}"))
}

/// A body with its pictures at their full addresses, and the ones the
/// page may not show left out.
fn with_pictures(body: &str) -> String {
    body.split("\n\n")
        .filter_map(|paragraph| match picture(paragraph) {
            Some(p) => picture_url(&p.url).map(|url| format!("![{}]({url})", p.caption)),
            None => Some(paragraph.to_owned()),
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

/// A body with the pictures of a post's `pictures` key put in: each
/// `{ file, caption, after }` goes after paragraph number `after` of the
/// body (0 before it, none or past its end after it). Versions of
/// Renkatsu before pictures ignore the key and show the body alone.
fn place_pictures(body: &str, list: Option<&toml::Value>) -> String {
    let listed: Vec<(usize, String)> = list
        .and_then(toml::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|p| {
            let file = p.get("file")?.as_str()?;
            let caption = p.get("caption").and_then(toml::Value::as_str).unwrap_or_default();
            let after = p
                .get("after")
                .and_then(toml::Value::as_integer)
                .map_or(usize::MAX, |n| n.max(0) as usize);
            Some((after, format!("![{}]({})", plain(caption, TITLE_CHARS), file.trim())))
        })
        .collect();
    if listed.is_empty() {
        return body.to_owned();
    }
    let paragraphs: Vec<&str> = body.split("\n\n").filter(|p| !p.is_empty()).collect();
    let mut out: Vec<String> = Vec::new();
    for at in 0..=paragraphs.len() {
        let last = at == paragraphs.len();
        out.extend(
            listed
                .iter()
                .filter(|(after, _)| *after == at || (last && *after > at))
                .map(|(_, shown)| shown.clone()),
        );
        if let Some(p) = paragraphs.get(at) {
            out.push((*p).to_owned());
        }
    }
    out.join("\n\n")
}

/// The picture a line of release notes is: markdown's `![alt](address)`
/// or the `<img>` GitHub writes for a pasted one; as a paragraph of the
/// body.
fn release_picture(line: &str) -> Option<String> {
    let line = line.trim();
    if let Some(p) = picture(line) {
        return Some(format!("![{}]({})", p.caption, p.url));
    }
    if !line.starts_with("<img") {
        return None;
    }
    let attribute = |name: &str| {
        let at = line.find(&format!("{name}=\""))? + name.len() + 2;
        let end = line[at..].find('"')?;
        Some(line[at..at + end].to_owned())
    };
    let src = attribute("src")?;
    let alt = attribute("alt").unwrap_or_default();
    let alt = if alt.eq_ignore_ascii_case("image") {
        String::new()
    } else {
        alt
    };
    Some(format!("![{}]({src})", plain(&alt, TITLE_CHARS)))
}

/// Where a piece of news comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Kind {
    /// A release on GitHub.
    Release,
    /// A post of the news file.
    Post,
}

/// The language the news is read in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Language {
    #[default]
    English,
    Japanese,
}

/// One piece of news.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Post {
    pub kind: Kind,
    /// The day, as `2026-11-16`.
    pub date: String,
    pub title: String,
    #[serde(default)]
    pub summary: String,
    /// The page a click opens; `None` when the post is no link.
    #[serde(default)]
    pub url: Option<String>,
    /// The Japanese version, where the post has one.
    #[serde(default)]
    pub title_ja: Option<String>,
    #[serde(default)]
    pub summary_ja: Option<String>,
    /// The post itself, paragraphs separated by an empty line; empty when
    /// the post is its summary alone.
    #[serde(default)]
    pub body: String,
    #[serde(default)]
    pub body_ja: Option<String>,
    /// Who wrote it: a post's `author`, the account that published a
    /// release.
    #[serde(default)]
    pub author: Option<String>,
}

impl Post {
    /// The post's text in a language: its body, or its summary when it has
    /// none.
    pub fn body_in(&self, language: Language) -> &str {
        match (language, &self.body_ja) {
            (Language::Japanese, Some(body)) => body,
            _ if !self.body.is_empty() => &self.body,
            _ => self.summary_in(language),
        }
    }

    /// The title in a language, in English where the post has no other.
    pub fn title_in(&self, language: Language) -> &str {
        match (language, &self.title_ja) {
            (Language::Japanese, Some(title)) => title,
            _ => &self.title,
        }
    }

    pub fn summary_in(&self, language: Language) -> &str {
        match (language, &self.summary_ja) {
            (Language::Japanese, Some(summary)) => summary,
            _ => &self.summary,
        }
    }

    /// The day as the page shows it: "16 Nov", with the year when it is
    /// not `today`'s.
    pub fn day(&self, today: &str) -> String {
        const MONTHS: [&str; 12] = [
            "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
        ];
        let part = |range: std::ops::Range<usize>| self.date.get(range).and_then(|p| p.parse::<usize>().ok());
        let (Some(year), Some(month), Some(day)) = (part(0..4), part(5..7), part(8..10)) else {
            return self.date.clone();
        };
        let month = MONTHS[(month.max(1) - 1) % 12];
        if today.get(0..4) == self.date.get(0..4) {
            format!("{day} {month}")
        } else {
            format!("{day} {month} {year}")
        }
    }
}

/// A day written as `2026-11-16`, or nothing when the text is no day.
fn day_of(text: &str) -> Option<String> {
    let day = text.get(0..10)?;
    let shape = day.bytes().enumerate().all(|(n, b)| {
        if n == 4 || n == 7 {
            b == b'-'
        } else {
            b.is_ascii_digit()
        }
    });
    shape.then(|| day.to_owned())
}

/// The day of a time in seconds since 1970, as `2026-11-16`.
pub fn today(seconds: u64) -> String {
    // Days to a date of the calendar, after Howard Hinnant's algorithm.
    let z = (seconds / 86_400) as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02}")
}

/// Plain text of one line, cut to a length.
fn plain(text: &str, chars: usize) -> String {
    let line: Vec<&str> = text.split_whitespace().collect();
    let line: String = line.join(" ").chars().filter(|c| !c.is_control()).collect();
    if line.chars().count() <= chars {
        return line;
    }
    let cut: String = line.chars().take(chars - 1).collect();
    format!("{}…", cut.trim_end())
}

/// Text in paragraphs: each paragraph on one line, paragraphs apart by an
/// empty line, cut at `chars`.
fn paragraphs(text: &str, chars: usize) -> String {
    let mut out: Vec<String> = Vec::new();
    for block in text.replace("\r\n", "\n").split("\n\n") {
        let line = plain(block, usize::MAX);
        if !line.is_empty() {
            out.push(line);
        }
    }
    let text = out.join("\n\n");
    if text.chars().count() <= chars {
        return text;
    }
    let cut: String = text.chars().take(chars - 1).collect();
    format!("{}…", cut.trim_end())
}

/// A link the page may open: one into the project's own GitHub.
fn link(url: Option<&str>) -> Option<String> {
    url.filter(|u| u.starts_with(LINKS) && !u.contains(char::is_whitespace))
        .map(str::to_owned)
}

/// Reads the news file. Posts for a newer Renkatsu than `running`, and
/// posts whose last day is before `today`, are left out.
pub fn parse_posts(text: &str, running: &str, today: &str) -> Result<Vec<Post>> {
    if text.len() > MAX_BYTES {
        bail!("the news file is larger than {} KB", MAX_BYTES / 1024);
    }
    let file: toml::Table = text.parse().context("reading the news file")?;
    let Some(list) = file.get("post").and_then(toml::Value::as_array) else {
        return Ok(Vec::new());
    };
    let mut posts = Vec::new();
    for entry in list {
        let text = |key: &str| entry.get(key).and_then(toml::Value::as_str);
        // A date is written bare or in quotes.
        let date = |key: &str| match entry.get(key) {
            Some(toml::Value::Datetime(d)) => day_of(&d.to_string()),
            Some(toml::Value::String(s)) => day_of(s),
            _ => None,
        };
        let (Some(day), Some(title)) = (date("date"), text("title").map(|t| plain(t, TITLE_CHARS))) else {
            continue;
        };
        if title.is_empty() {
            continue;
        }
        if text("since").is_some_and(|since| version_key(since) > version_key(running)) {
            continue;
        }
        if date("until").is_some_and(|until| until.as_str() < today) {
            continue;
        }
        let short = |key: &str, chars| text(key).map(|t| plain(t, chars)).filter(|t| !t.is_empty());
        posts.push(Post {
            kind: Kind::Post,
            date: day,
            title,
            summary: short("summary", SUMMARY_CHARS).unwrap_or_default(),
            url: link(text("url")),
            title_ja: short("title_ja", TITLE_CHARS),
            summary_ja: short("summary_ja", SUMMARY_CHARS),
            // A post with pictures and no body shows its summary above them.
            body: text("body")
                .or(entry.get("pictures").and(text("summary")))
                .map(|t| with_pictures(&place_pictures(&paragraphs(t, BODY_CHARS), entry.get("pictures"))))
                .unwrap_or_default(),
            body_ja: text("body_ja")
                .map(|t| with_pictures(&place_pictures(&paragraphs(t, BODY_CHARS), entry.get("pictures"))))
                .filter(|t| !t.is_empty()),
            author: short("author", AUTHOR_CHARS),
        });
    }
    Ok(posts)
}

/// Reads GitHub's list of releases: the releases of Renkatsu itself as
/// news, and the newest of them. Drafts, prereleases and releases of
/// content, whose tags do not start with `v`, are left out.
pub fn parse_releases(json: &str) -> Result<(Vec<Post>, Option<Release>)> {
    let list: serde_json::Value = serde_json::from_str(json).context("reading the releases")?;
    let list = list.as_array().context("the releases are not a list")?;
    let mut posts = Vec::new();
    let mut latest: Option<Release> = None;
    for release in list {
        let Some(tag) = release["tag_name"].as_str().filter(|t| t.starts_with('v')) else {
            continue;
        };
        if release["draft"].as_bool() == Some(true) || release["prerelease"].as_bool() == Some(true) {
            continue;
        }
        let version = tag.trim_start_matches('v').to_owned();
        let url = release["html_url"].as_str().unwrap_or_default().to_owned();
        if latest
            .as_ref()
            .is_none_or(|l| version_key(&version) > version_key(&l.version))
        {
            latest = Some(Release {
                version: version.clone(),
                url: url.clone(),
            });
        }
        let Some(day) = release["published_at"].as_str().and_then(day_of) else {
            continue;
        };
        let notes = release["body"].as_str().unwrap_or_default();
        // The notes a line each, without their markdown marks but for a
        // heading's, which the page draws as one, and without the section
        // of files to download, which the page has no use for.
        let mut downloads = false;
        let mut lines: Vec<String> = Vec::new();
        let mut summary = "";
        for line in notes.lines() {
            // A picture is a paragraph of its own, never the summary.
            if let Some(shown) = release_picture(line) {
                if !downloads {
                    lines.push(shown);
                }
                continue;
            }
            let heading = line.starts_with('#');
            let text = line.trim_start_matches(['#', '*', '-', ' ']).trim();
            if heading {
                downloads = text.eq_ignore_ascii_case("downloads");
            }
            if downloads || text.is_empty() {
                continue;
            }
            // The first line that says something is the summary.
            if summary.is_empty() {
                summary = text;
            }
            lines.push(if heading {
                format!("{HEADING}{text}")
            } else {
                text.to_owned()
            });
        }
        let body = with_pictures(&paragraphs(&lines.join("\n\n"), BODY_CHARS));
        posts.push(Post {
            kind: Kind::Release,
            date: day,
            title: format!("Renkatsu {version}"),
            summary: plain(summary, SUMMARY_CHARS),
            url: link(Some(&url)),
            title_ja: None,
            summary_ja: None,
            body,
            body_ja: None,
            author: release["author"]["login"]
                .as_str()
                .map(|a| plain(a, AUTHOR_CHARS))
                .filter(|a| !a.is_empty()),
        });
    }
    Ok((posts, latest))
}

/// The posts and the releases as one list: the newest first, a post
/// before a release of the same day, and no more than the page shows.
pub fn merge(posts: Vec<Post>, releases: Vec<Post>) -> Vec<Post> {
    let mut all = posts;
    all.extend(releases);
    // The sort is stable, so the order within a day stays.
    all.sort_by(|a, b| b.date.cmp(&a.date));
    all.truncate(SHOWN);
    all
}

/// What the last answers said, kept between runs.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Cache {
    /// The Renkatsu that wrote it. Another version reads the answers in
    /// its own way, so it starts from an empty cache and asks again.
    pub app: String,
    /// When the news file was last asked for, in seconds since 1970, and
    /// the same for the releases.
    pub posts_asked: u64,
    pub releases_asked: u64,
    pub posts_etag: Option<String>,
    pub releases_etag: Option<String>,
    /// The news file as it came.
    pub posts: String,
    pub releases: Vec<Post>,
    pub latest_version: Option<String>,
    pub latest_url: Option<String>,
}

/// What the program wants to know.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Wanted {
    pub news: bool,
    pub updates: bool,
}

/// The news to show and the newer Renkatsu to tell of.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Feed {
    pub posts: Vec<Post>,
    pub update: Option<Release>,
}

/// Whether an answer of `asked` is still good at `now`.
pub fn fresh(asked: u64, now: u64) -> bool {
    asked != 0 && now.saturating_sub(asked) < FRESH
}

impl Cache {
    /// The file the cache is kept in.
    pub fn file() -> PathBuf {
        let base = std::env::var_os("XDG_CACHE_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("LOCALAPPDATA").map(PathBuf::from))
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cache")))
            .unwrap_or_else(std::env::temp_dir);
        base.join("renkatsu").join("news.json")
    }

    /// The cache of a file; an empty one when the file is missing, cannot
    /// be read or was written by another version of Renkatsu.
    pub fn load(file: &Path) -> Cache {
        std::fs::read_to_string(file)
            .ok()
            .and_then(|text| Self::read(&text))
            .unwrap_or_default()
    }

    /// The cache in `text`, when this version of Renkatsu wrote it.
    fn read(text: &str) -> Option<Cache> {
        serde_json::from_str::<Cache>(text)
            .ok()
            .filter(|cache| cache.app == crate::APP_VERSION)
    }

    pub fn save(&self, file: &Path) -> Result<()> {
        if let Some(dir) = file.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let stamped = Cache {
            app: crate::APP_VERSION.to_owned(),
            ..self.clone()
        };
        std::fs::write(file, serde_json::to_string(&stamped)?)?;
        Ok(())
    }

    /// What the cache has of what is wanted, for a program of version
    /// `running` at `now`.
    pub fn feed(&self, wanted: Wanted, running: &str, now: u64) -> Feed {
        let mut feed = Feed::default();
        if wanted.news {
            let posts = parse_posts(&self.posts, running, &today(now)).unwrap_or_default();
            feed.posts = merge(posts, self.releases.clone());
        }
        if wanted.updates {
            if let (Some(version), Some(url)) = (&self.latest_version, &self.latest_url) {
                let release = Release {
                    version: version.clone(),
                    url: url.clone(),
                };
                feed.update = crate::update::newer(release, running);
            }
        }
        feed
    }

    /// Asks for what is wanted and no longer fresh. Returns whether
    /// anything was asked. An answer that fails, offline or over GitHub's
    /// limit, leaves what the cache has.
    pub fn refresh(&mut self, wanted: Wanted, now: u64) -> bool {
        let mut asked = false;
        if (wanted.news || wanted.updates) && !fresh(self.releases_asked, now) {
            asked = true;
            match get(RELEASES, self.releases_etag.as_deref(), 1 << 20) {
                Ok(Answer::Same) => self.releases_asked = now,
                Ok(Answer::New(text, etag)) => {
                    if let Ok((releases, latest)) = parse_releases(&text) {
                        self.releases = releases;
                        self.latest_version = latest.as_ref().map(|l| l.version.clone());
                        self.latest_url = latest.map(|l| l.url);
                        self.releases_etag = etag;
                        self.releases_asked = now;
                    }
                }
                Err(_) => {}
            }
        }
        if wanted.news && !fresh(self.posts_asked, now) {
            asked = true;
            match get(POSTS, self.posts_etag.as_deref(), MAX_BYTES as u64 + 1) {
                Ok(Answer::Same) => self.posts_asked = now,
                Ok(Answer::New(text, etag)) => {
                    // A file that does not read is not kept over a good one.
                    if parse_posts(&text, "0", "0000-00-00").is_ok() {
                        self.posts = text;
                        self.posts_etag = etag;
                        self.posts_asked = now;
                    }
                }
                Err(_) => {}
            }
        }
        asked
    }
}

/// The file of a picture of the news, fetched once and kept in the cache
/// folder beside the news. Only the news folder's pictures and those of
/// releases are fetched, at most `PICTURE_BYTES`, and only PNG and JPEG.
pub fn picture_file(url: &str) -> Result<PathBuf> {
    if picture_url(url).as_deref() != Some(url) {
        bail!("{url} is not a picture of the news");
    }
    // FNV-1a of the address names the file.
    let name = url.bytes().fold(0xcbf2_9ce4_8422_2325_u64, |h, b| {
        (h ^ u64::from(b)).wrapping_mul(0x100_0000_01b3)
    });
    let dir = Cache::file().with_file_name("news-pictures");
    let file = dir.join(format!("{name:016x}"));
    if file.is_file() {
        return Ok(file);
    }
    let mut answer = crate::download::agent()
        .get(url)
        .call()
        .with_context(|| format!("asking for {url}"))?;
    if !answer.status().is_success() {
        bail!("{url} answered {}", answer.status());
    }
    let mut bytes = Vec::new();
    answer
        .body_mut()
        .with_config()
        .limit(PICTURE_BYTES + 1)
        .reader()
        .take(PICTURE_BYTES + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > PICTURE_BYTES {
        bail!("{url} is larger than {} MB", PICTURE_BYTES >> 20);
    }
    if !(bytes.starts_with(b"\x89PNG") || bytes.starts_with(&[0xff, 0xd8, 0xff])) {
        bail!("{url} is not a PNG or JPEG picture");
    }
    std::fs::create_dir_all(&dir)?;
    let part = file.with_extension("part");
    std::fs::write(&part, &bytes)?;
    std::fs::rename(&part, &file)?;
    Ok(file)
}

pub(crate) enum Answer {
    /// What the cache has is still what the server has.
    Same,
    /// The text and its ETag.
    New(String, Option<String>),
}

/// Asks for a page, with the ETag of the copy at hand. The request names
/// the program and its version and nothing else.
pub(crate) fn get(url: &str, etag: Option<&str>, limit: u64) -> Result<Answer> {
    let mut request = crate::download::agent()
        .get(url)
        .header("Accept", "application/vnd.github+json");
    if let Some(etag) = etag {
        request = request.header("If-None-Match", etag);
    }
    let mut answer = request.call().with_context(|| format!("asking for {url}"))?;
    if answer.status().as_u16() == 304 {
        return Ok(Answer::Same);
    }
    if !answer.status().is_success() {
        bail!("{url} answered {}", answer.status());
    }
    let etag = answer
        .headers()
        .get("etag")
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned);
    let mut text = String::new();
    answer.body_mut().as_reader().take(limit).read_to_string(&mut text)?;
    Ok(Answer::New(text, etag))
}

#[cfg(test)]
mod tests {
    use super::*;

    const FILE: &str = r#"
        [[post]]
        date = 2026-11-18
        title = "Starter Content is out"
        summary = "Characters and clothing\nfor those   without Koikatsu."
        url = "https://github.com/Mihnea8848/renkatsu/releases/tag/starter-characters-1"
        title_ja = "スターターコンテンツ"
        colour = "ignored"

        [[post]]
        date = "2026-11-16"
        title = "Posing comes next"
        url = "https://example.org/elsewhere"

        [[post]]
        date = 2026-11-15
        title = "For a later Renkatsu"
        since = "0.3.0"

        [[post]]
        date = 2026-11-01
        title = "Over by now"
        until = 2026-11-10

        [[post]]
        title = "No date"
    "#;

    const RELEASES_JSON: &str = r##"[
        {"tag_name": "starter-characters-1", "html_url": "https://github.com/Mihnea8848/renkatsu/releases/tag/starter-characters-1",
         "published_at": "2026-11-18T10:00:00Z", "body": "Content"},
        {"tag_name": "v0.1.8", "html_url": "https://github.com/Mihnea8848/renkatsu/releases/tag/v0.1.8",
         "published_at": "2026-11-18T09:00:00Z", "body": "\n## Gizmos, viewport selection and shortcuts\n\nMore text\n\n## Downloads\n\n- Linux: the installer\n\n## After\n\nLast words"},
        {"tag_name": "v0.2.0", "html_url": "https://github.com/Mihnea8848/renkatsu/releases/tag/v0.2.0",
         "published_at": "2026-11-19T09:00:00Z", "body": "", "draft": true},
        {"tag_name": "v0.1.10", "html_url": "https://github.com/Mihnea8848/renkatsu/releases/tag/v0.1.10",
         "published_at": "2026-11-17T09:00:00Z", "body": "Fixes", "prerelease": false}
    ]"##;

    #[test]
    fn a_post_keeps_its_paragraphs() {
        let text = "[[post]]\ndate = 2026-11-19\ntitle = \"Maps\"\nsummary = \"Short.\"\nbody = \"\"\"\nFirst   line\nsame paragraph.\n\nSecond paragraph.\n\"\"\"\n";
        let posts = parse_posts(text, "0.1.8", "2026-11-19").unwrap();
        assert_eq!(posts[0].body, "First line same paragraph.\n\nSecond paragraph.");
        assert_eq!(posts[0].body_in(Language::English), posts[0].body);
        // Without a body the summary stands in.
        let text = "[[post]]\ndate = 2026-11-19\ntitle = \"Maps\"\nsummary = \"Short.\"\n";
        let posts = parse_posts(text, "0.1.8", "2026-11-19").unwrap();
        assert_eq!(posts[0].body_in(Language::English), "Short.");
    }

    #[test]
    fn posts_are_read_filtered_and_kept_plain() {
        let posts = parse_posts(FILE, "0.1.8", "2026-11-19").unwrap();
        let titles: Vec<&str> = posts.iter().map(|p| p.title.as_str()).collect();
        assert_eq!(titles, ["Starter Content is out", "Posing comes next"]);
        assert_eq!(posts[0].date, "2026-11-18");
        assert_eq!(posts[0].summary, "Characters and clothing for those without Koikatsu.");
        assert!(posts[0].url.is_some());
        // A link out of the project's GitHub is shown without its link.
        assert_eq!(posts[1].url, None);
        assert_eq!(posts[1].date, "2026-11-16");
        // The Japanese version shows where it is asked for and there is one.
        assert_eq!(posts[0].title_in(Language::Japanese), "スターターコンテンツ");
        assert_eq!(posts[0].title_in(Language::English), "Starter Content is out");
        assert_eq!(posts[1].title_in(Language::Japanese), "Posing comes next");
        assert_eq!(posts[0].summary_in(Language::Japanese), posts[0].summary);
        // A newer program sees the post that waited for it.
        assert_eq!(parse_posts(FILE, "0.3.0", "2026-11-19").unwrap().len(), 3);
        // Before its last day a post still shows.
        assert_eq!(parse_posts(FILE, "0.1.8", "2026-11-10").unwrap().len(), 3);
    }

    #[test]
    fn a_bad_news_file_is_refused() {
        assert!(parse_posts("[[post", "0.1.8", "2026-11-19").is_err());
        let large = format!("# {}\n", "x".repeat(MAX_BYTES));
        assert!(parse_posts(&large, "0.1.8", "2026-11-19").is_err());
        assert_eq!(parse_posts("", "0.1.8", "2026-11-19").unwrap(), []);
        let long = format!("[[post]]\ndate = 2026-01-01\ntitle = \"{}\"", "a".repeat(500));
        let posts = parse_posts(&long, "0.1.8", "2026-11-19").unwrap();
        assert_eq!(posts[0].title.chars().count(), TITLE_CHARS);
        assert!(posts[0].title.ends_with('…'));
    }

    #[test]
    fn releases_give_the_news_and_the_newest_version() {
        let (posts, latest) = parse_releases(RELEASES_JSON).unwrap();
        let titles: Vec<&str> = posts.iter().map(|p| p.title.as_str()).collect();
        assert_eq!(titles, ["Renkatsu 0.1.8", "Renkatsu 0.1.10"]);
        assert_eq!(posts[0].summary, "Gizmos, viewport selection and shortcuts");
        // The files to download are left out; what follows them is kept.
        assert_eq!(
            posts[0].body,
            "## Gizmos, viewport selection and shortcuts\n\nMore text\n\n## After\n\nLast words"
        );
        assert_eq!(posts[0].date, "2026-11-18");
        // The newest by version, not by its place in the list.
        assert_eq!(latest.unwrap().version, "0.1.10");
        assert!(parse_releases("{}").is_err());
        assert_eq!(parse_releases("[]").unwrap(), (vec![], None));
    }

    #[test]
    fn news_merge_newest_first_and_stop_at_what_shows() {
        let posts = parse_posts(FILE, "0.1.8", "2026-11-19").unwrap();
        let (releases, _) = parse_releases(RELEASES_JSON).unwrap();
        let all = merge(posts.clone(), releases.clone());
        let titles: Vec<&str> = all.iter().map(|p| p.title.as_str()).collect();
        assert_eq!(
            titles,
            [
                "Starter Content is out",
                "Renkatsu 0.1.8",
                "Renkatsu 0.1.10",
                "Posing comes next"
            ]
        );
        let many: Vec<Post> = (0..20).map(|_| posts[0].clone()).collect();
        assert_eq!(merge(many, releases).len(), SHOWN);
    }

    #[test]
    fn an_answer_is_fresh_for_a_quarter_of_an_hour() {
        assert!(!fresh(0, 1000));
        assert!(fresh(1000, 1000));
        assert!(fresh(1000, 1000 + FRESH - 1));
        assert!(!fresh(1000, 1000 + FRESH));
        // A clock that went back does not ask again at once.
        assert!(fresh(2000, 1000));
    }

    #[test]
    fn the_cache_feeds_what_is_wanted() {
        let (releases, latest) = parse_releases(RELEASES_JSON).unwrap();
        let latest = latest.unwrap();
        let cache = Cache {
            posts: FILE.to_owned(),
            releases,
            latest_version: Some(latest.version),
            latest_url: Some(latest.url),
            ..Default::default()
        };
        let now = 1_795_046_400; // 2026-11-19
        assert_eq!(today(now), "2026-11-19");
        let both = Wanted {
            news: true,
            updates: true,
        };
        let feed = cache.feed(both, "0.1.8", now);
        assert_eq!(feed.posts.len(), 4);
        assert_eq!(feed.update.as_ref().unwrap().version, "0.1.10");
        // The running version is the newest: nothing to tell.
        assert_eq!(cache.feed(both, "0.1.10", now).update, None);
        let quiet = Wanted {
            news: false,
            updates: false,
        };
        assert_eq!(cache.feed(quiet, "0.1.8", now), Feed::default());
        // The cache survives a round through its file's format.
        let text = serde_json::to_string(&cache).unwrap();
        assert_eq!(serde_json::from_str::<Cache>(&text).unwrap(), cache);
        assert_eq!(serde_json::from_str::<Cache>("{}").unwrap(), Cache::default());
    }

    #[test]
    fn a_cache_of_another_version_is_not_read() {
        let mine = Cache {
            app: crate::APP_VERSION.to_owned(),
            latest_version: Some("9.9.9".to_owned()),
            ..Default::default()
        };
        let text = serde_json::to_string(&mine).unwrap();
        assert_eq!(Cache::read(&text), Some(mine));
        // One from before the version was kept, or from another version.
        assert_eq!(Cache::read(r#"{"latest_version": "0.1.8"}"#), None);
        assert_eq!(Cache::read(r#"{"app": "0.0.1", "latest_version": "0.1.8"}"#), None);
    }

    #[test]
    fn days_read_as_the_page_shows_them() {
        let post = |date: &str| Post {
            kind: Kind::Post,
            date: date.to_owned(),
            title: String::new(),
            summary: String::new(),
            url: None,
            title_ja: None,
            summary_ja: None,
            body: String::new(),
            body_ja: None,
            author: None,
        };
        assert_eq!(post("2026-11-06").day("2026-11-19"), "6 Nov");
        assert_eq!(post("2025-12-31").day("2026-01-02"), "31 Dec 2025");
        assert_eq!(today(0), "1970-01-01");
        assert_eq!(today(951_782_400), "2000-02-29");
    }

    #[test]
    fn pictures_of_a_post_get_their_full_address_and_others_are_left_out() {
        let file = r#"
            [[post]]
            date = 2026-11-24
            title = "Mirror pose"
            body = """
            Mirror flips a pose.

            ![Before and after](images/mirror.png)

            ![Elsewhere](https://example.org/x.png)

            ![Up a folder](../secret.png)

            ![Full address](https://raw.githubusercontent.com/Mihnea8848/renkatsu/main/news/images/a.png)
            """
        "#;
        let posts = parse_posts(file, "1.0.0", "2026-11-24").unwrap();
        let shown: Vec<Picture> = posts[0].body.split("\n\n").filter_map(picture).collect();
        assert_eq!(
            shown,
            [
                Picture {
                    url: format!("{PICTURES}images/mirror.png"),
                    caption: "Before and after".into()
                },
                Picture {
                    url: format!("{PICTURES}images/a.png"),
                    caption: "Full address".into()
                },
            ]
        );
        assert!(posts[0].body.starts_with("Mirror flips a pose."));
        assert_eq!(picture("A line with ![a](b.png) in it"), None);
    }

    #[test]
    fn a_picture_pasted_into_release_notes_is_shown_but_is_never_the_summary() {
        let json = r#"[{"tag_name": "v0.3.0", "html_url": "https://github.com/Mihnea8848/renkatsu/releases/tag/v0.3.0",
            "published_at": "2026-11-30T10:00:00Z",
            "body": "<img width=\"1600\" alt=\"Posing\" src=\"https://github.com/user-attachments/assets/1f2e\" />\n\nPosing by hand is here.\n\n![x](https://example.org/y.png)"}]"#;
        let (posts, _) = parse_releases(json).unwrap();
        assert_eq!(posts[0].summary, "Posing by hand is here.");
        let shown: Vec<Picture> = posts[0].body.split("\n\n").filter_map(picture).collect();
        assert_eq!(
            shown,
            [Picture {
                url: "https://github.com/user-attachments/assets/1f2e".into(),
                caption: "Posing".into()
            }]
        );
    }

    #[test]
    fn only_pictures_of_the_news_are_fetched() {
        assert!(picture_file("https://example.org/a.png").is_err());
        assert!(picture_file(&format!("{PICTURES}../x.png")).is_err());
    }

    #[test]
    fn listed_pictures_go_after_the_paragraph_they_name() {
        let file = r#"
            [[post]]
            date = 2026-11-18
            title = "Starter Content"
            body = """
            One.

            Two.
            """
            pictures = [
                { file = "images/end.png" },
                { file = "images/first.png", caption = "Before all", after = 0 },
                { file = "images/two.png", caption = "After one", after = 1 },
            ]

            [[post]]
            date = 2026-11-17
            title = "Summary only"
            summary = "Short."
            pictures = [{ file = "images/s.png" }]
        "#;
        let posts = parse_posts(file, "1.0.0", "2026-11-18").unwrap();
        let order: Vec<String> = posts[0]
            .body
            .split("\n\n")
            .map(|p| picture(p).map_or(p.to_owned(), |p| p.url.trim_start_matches(PICTURES).to_owned()))
            .collect();
        assert_eq!(
            order,
            ["images/first.png", "One.", "images/two.png", "Two.", "images/end.png"]
        );
        assert_eq!(
            picture(posts[0].body.split("\n\n").nth(2).unwrap()).unwrap().caption,
            "After one"
        );
        assert_eq!(posts[1].body, format!("Short.\n\n![]({PICTURES}images/s.png)"));
    }

    #[test]
    fn a_post_says_who_wrote_it() {
        let posts = parse_posts(
            "[[post]]\ndate = 2026-11-24\ntitle = \"T\"\nauthor = \"Mihnea8848\"\n",
            "1.0.0",
            "2026-11-24",
        )
        .unwrap();
        assert_eq!(posts[0].author.as_deref(), Some("Mihnea8848"));
        let json = r#"[{"tag_name": "v0.3.0", "published_at": "2026-11-30T10:00:00Z", "author": {"login": "Mihnea8848"}, "body": "x"}]"#;
        assert_eq!(parse_releases(json).unwrap().0[0].author.as_deref(), Some("Mihnea8848"));
    }
}
