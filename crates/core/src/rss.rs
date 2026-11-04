use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{LazyLock, Mutex, MutexGuard};
use std::time::Duration;

use anyhow::{Result, anyhow, bail};
use regex::{Regex, RegexBuilder};
use reqwest::{Client, Url};
use roxmltree::{Document, Node, ParsingOptions};

use crate::api::{Feed, FeedItem, Rss, Rule, RuleMatch};

/// Items kept per feed beyond those it still lists.
const KEEP: usize = 100;
/// The shortest wait between checks of one feed, so a typo in the settings cannot hammer a server.
const MIN_MINUTES: u32 = 5;

/// One torrent a rule picked from a feed.
pub struct Take {
    pub feed: String,
    pub title: String,
    pub link: String,
    pub category: Option<String>,
    pub save_path: Option<String>,
    pub paused: bool,
}

/// The RSS feeds and download rules, saved in `rss.json`.
pub struct Feeds {
    path: PathBuf,
    http: Client,
    state: Mutex<Rss>,
    rev: AtomicU64,
    busy: Mutex<HashSet<String>>,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

impl Feeds {
    pub fn load(path: PathBuf) -> Self {
        let state = std::fs::read(&path)
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default();
        Self {
            path,
            http: client(),
            state: Mutex::new(state),
            rev: AtomicU64::new(1),
            busy: Mutex::default(),
        }
    }

    pub fn rev(&self) -> u64 {
        self.rev.load(Ordering::Relaxed)
    }

    pub fn get(&self) -> Rss {
        lock(&self.state).clone()
    }

    fn edit<R>(&self, edit: impl FnOnce(&mut Rss) -> R) -> R {
        let mut state = lock(&self.state);
        let result = edit(&mut state);
        let saved = serde_json::to_vec_pretty(&*state)
            .map_err(std::io::Error::other)
            .and_then(|json| crate::store::write_atomic(&self.path, &json));
        if let Err(err) = saved {
            tracing::warn!("cannot save rss.json: {err}");
        }
        self.rev.fetch_add(1, Ordering::Relaxed);
        result
    }

    pub fn ids(&self) -> Vec<String> {
        lock(&self.state)
            .feeds
            .iter()
            .map(|feed| feed.id.clone())
            .collect()
    }

    pub fn add_feed(&self, url: &str) -> Result<String> {
        let url = url.trim();
        let parsed = Url::parse(url).map_err(|_| anyhow!("{url} is not a web address"))?;
        if !matches!(parsed.scheme(), "http" | "https") {
            bail!("{url} is not a web address");
        }
        self.edit(|rss| {
            if rss.feeds.iter().any(|feed| feed.url == url) {
                bail!("that feed is already there");
            }
            let id = new_id();
            rss.feeds.push(Feed {
                id: id.clone(),
                url: url.to_string(),
                ..Feed::default()
            });
            Ok(id)
        })
    }

    pub fn update_feed(&self, id: &str, name: String, enabled: bool) -> Result<()> {
        self.edit(|rss| {
            let feed = rss
                .feeds
                .iter_mut()
                .find(|feed| feed.id == id)
                .ok_or_else(|| anyhow!("that feed is gone"))?;
            feed.name = name.trim().to_string();
            feed.enabled = enabled;
            Ok(())
        })
    }

    /// A rule left with no feeds would watch every feed, so it is turned off instead.
    pub fn remove_feed(&self, id: &str) {
        self.edit(|rss| {
            rss.feeds.retain(|feed| feed.id != id);
            for rule in &mut rss.rules {
                if rule.feeds.iter().any(|feed| feed == id) {
                    rule.feeds.retain(|feed| feed != id);
                    if rule.feeds.is_empty() {
                        rule.enabled = false;
                    }
                }
            }
        });
    }

    pub fn save_rule(&self, mut rule: Rule) -> Result<String> {
        Matcher::new(&rule)?;
        rule.name = rule.name.trim().to_string();
        if rule.name.is_empty() {
            bail!("name the rule");
        }
        rule.category = rule.category.filter(|name| !name.trim().is_empty());
        rule.save_path = rule
            .save_path
            .map(|path| path.trim().to_string())
            .filter(|path| !path.is_empty());
        self.edit(|rss| {
            match rss.rules.iter_mut().find(|old| old.id == rule.id) {
                Some(old) if !rule.id.is_empty() => {
                    rule.episodes = std::mem::take(&mut old.episodes);
                    rule.last_match = old.last_match;
                    *old = rule.clone();
                }
                _ => {
                    rule.id = new_id();
                    rss.rules.push(rule.clone());
                }
            }
            Ok(rule.id)
        })
    }

    pub fn remove_rule(&self, id: &str) {
        self.edit(|rss| rss.rules.retain(|rule| rule.id != id));
    }

    pub fn rename_category(&self, from: &str, to: &str) {
        if !lock(&self.state)
            .rules
            .iter()
            .any(|rule| rule.category.as_deref() == Some(from))
        {
            return;
        }
        self.edit(|rss| {
            for rule in &mut rss.rules {
                if rule.category.as_deref() == Some(from) {
                    rule.category = Some(to.to_string());
                }
            }
        });
    }

    /// Adds feeds whose address is new and rules whose id is new, and replaces rules it has.
    pub fn restore(&self, feeds: Vec<Feed>, rules: Vec<Rule>) {
        self.edit(|rss| {
            for mut feed in feeds {
                if rss.feeds.iter().any(|old| old.url == feed.url) {
                    continue;
                }
                if feed.id.is_empty() || rss.feeds.iter().any(|old| old.id == feed.id) {
                    feed.id = new_id();
                }
                feed.items.clear();
                feed.checked = None;
                feed.error = None;
                feed.ready = false;
                rss.feeds.push(feed);
            }
            for mut rule in rules {
                match rss.rules.iter_mut().find(|old| old.id == rule.id) {
                    Some(old) if !rule.id.is_empty() => *old = rule,
                    _ => {
                        rule.id = new_id();
                        rss.rules.push(rule);
                    }
                }
            }
        });
    }

    /// What `rule` would take from the items the feeds list now.
    pub fn matches(&self, rule: &Rule) -> Result<Vec<RuleMatch>> {
        let matcher = Matcher::new(rule)?;
        let state = lock(&self.state);
        Ok(state
            .feeds
            .iter()
            .filter(|feed| rule.feeds.is_empty() || rule.feeds.contains(&feed.id))
            .flat_map(|feed| {
                feed.items
                    .iter()
                    .filter(|item| !item.link.is_empty() && matcher.matches(&item.title))
                    .map(|item| RuleMatch {
                        feed: feed.id.clone(),
                        item: item.clone(),
                    })
            })
            .take(200)
            .collect())
    }

    /// Feeds whose next check is due; 0 minutes leaves checks to the person.
    pub fn due(&self, minutes: u32) -> Vec<String> {
        if minutes == 0 {
            return Vec::new();
        }
        let wait = i64::from(minutes.max(MIN_MINUTES)) * 60;
        let now = crate::now();
        let busy = lock(&self.busy);
        lock(&self.state)
            .feeds
            .iter()
            .filter(|feed| feed.enabled && !busy.contains(&feed.id))
            .filter(|feed| feed.checked.is_none_or(|at| now - at >= wait))
            .map(|feed| feed.id.clone())
            .collect()
    }

    pub fn note_error(&self, id: &str, error: String) {
        self.edit(|rss| {
            if let Some(feed) = rss.feeds.iter_mut().find(|feed| feed.id == id) {
                feed.error = Some(error);
            }
        });
    }

    /// Fetches one feed and returns what its rules take from the new items; the first check only learns what is there.
    pub async fn refresh(&self, id: &str) -> Vec<Take> {
        if !lock(&self.busy).insert(id.to_string()) {
            return Vec::new();
        }
        let url = lock(&self.state)
            .feeds
            .iter()
            .find(|feed| feed.id == id)
            .map(|feed| feed.url.clone());
        let fetched = match url {
            Some(url) => fetch(&self.http, &url).await,
            None => Err(anyhow!("that feed is gone")),
        };
        lock(&self.busy).remove(id);
        self.apply(id, fetched)
    }

    fn apply(&self, id: &str, fetched: Result<(String, Vec<FeedItem>)>) -> Vec<Take> {
        let now = crate::now();
        self.edit(|rss| {
            let Rss { feeds, rules } = rss;
            let Some(feed) = feeds.iter_mut().find(|feed| feed.id == id) else {
                return Vec::new();
            };
            feed.checked = Some(now);
            let (title, listed) = match fetched {
                Ok(parsed) => parsed,
                Err(err) => {
                    feed.error = Some(format!("{err:#}"));
                    return Vec::new();
                }
            };
            let first = !feed.ready;
            feed.ready = true;
            feed.error = None;
            feed.title = title;
            let known: HashSet<String> = feed.items.iter().map(|item| item.id.clone()).collect();
            let mut fresh: Vec<FeedItem> = listed
                .iter()
                .filter(|item| !known.contains(&item.id))
                .cloned()
                .collect();
            let mut takes = Vec::new();
            if !first {
                let watching: Vec<(usize, Matcher)> = rules
                    .iter()
                    .enumerate()
                    .filter(|(_, rule)| {
                        rule.enabled && (rule.feeds.is_empty() || rule.feeds.contains(&feed.id))
                    })
                    .filter_map(|(index, rule)| Some((index, Matcher::new(rule).ok()?)))
                    .collect();
                for item in fresh.iter_mut().rev().filter(|item| !item.link.is_empty()) {
                    for (index, matcher) in &watching {
                        let rule = &mut rules[*index];
                        if !matcher.matches(&item.title) {
                            continue;
                        }
                        let episode = rule.smart_episodes.then(|| episode(&item.title)).flatten();
                        if let Some(episode) = episode {
                            if rule.episodes.contains(&episode) {
                                continue;
                            }
                            rule.episodes.push(episode);
                        }
                        rule.last_match = Some(now);
                        item.rule = Some(rule.name.clone());
                        takes.push(Take {
                            feed: feed.id.clone(),
                            title: item.title.clone(),
                            link: item.link.clone(),
                            category: rule.category.clone(),
                            save_path: rule.save_path.clone(),
                            paused: rule.paused,
                        });
                        break;
                    }
                }
            }
            let listed: HashSet<&str> = listed.iter().map(|item| item.id.as_str()).collect();
            let mut items = fresh;
            items.extend(std::mem::take(&mut feed.items));
            let mut kept = 0;
            items.retain(|item| {
                kept += 1;
                kept <= KEEP || listed.contains(item.id.as_str())
            });
            feed.items = items;
            takes
        })
    }
}

fn new_id() -> String {
    let mut bytes = [0u8; 6];
    let _ = getrandom::fill(&mut bytes);
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn client() -> Client {
    Client::builder()
        .redirect(reqwest::redirect::Policy::limited(8))
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(30))
        .user_agent(format!("bawkbit/{}", crate::VERSION))
        .build()
        .expect("the HTTP client builds")
}

async fn fetch(client: &Client, url: &str) -> Result<(String, Vec<FeedItem>)> {
    let response = client.get(url).send().await.map_err(|err| {
        if err.is_timeout() {
            anyhow!("it took too long to answer")
        } else if err.is_connect() {
            anyhow!("cannot reach it")
        } else {
            anyhow!("{err}")
        }
    })?;
    let status = response.status();
    if !status.is_success() {
        bail!("it answered {status}");
    }
    let body = response.text().await?;
    parse(&body)
}

/// RSS 2.0, RSS 1.0 and Atom, with the Torznab and ezRSS extras torrent sites add.
pub fn parse(xml: &str) -> Result<(String, Vec<FeedItem>)> {
    let options = ParsingOptions {
        allow_dtd: true,
        ..ParsingOptions::default()
    };
    let doc = Document::parse_with_options(xml, options)
        .map_err(|_| anyhow!("it did not answer with a feed"))?;
    let root = doc.root_element();
    if !matches!(root.tag_name().name(), "rss" | "feed" | "RDF") {
        bail!("it did not answer with a feed");
    }
    let channel = child(root, "channel").unwrap_or(root);
    let title = text(channel, "title").unwrap_or_default();
    let mut seen = HashSet::new();
    let items = root
        .descendants()
        .filter(|node| node.is_element() && matches!(node.tag_name().name(), "item" | "entry"))
        .filter_map(item)
        .filter(|item| seen.insert(item.id.clone()))
        .collect();
    Ok((title, items))
}

fn item(node: Node) -> Option<FeedItem> {
    let title = text(node, "title").unwrap_or_default();
    let attr = |name: &str| {
        node.children()
            .filter(|child| child.is_element() && child.tag_name().name() == "attr")
            .find(|child| child.attribute("name") == Some(name))
            .and_then(|child| child.attribute("value"))
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(String::from)
    };
    let links: Vec<Node> = node
        .children()
        .filter(|child| child.is_element() && child.tag_name().name() == "link")
        .collect();
    let link_text = links
        .iter()
        .find_map(|link| link.text())
        .map(str::trim)
        .filter(|link| !link.is_empty());
    let href = |rel: Option<&str>| {
        links
            .iter()
            .filter(|link| link.attribute("rel") == rel)
            .find_map(|link| link.attribute("href"))
    };
    let enclosure = child(node, "enclosure");
    let enclosure_url = enclosure
        .and_then(|enclosure| enclosure.attribute("url"))
        .or_else(|| href(Some("enclosure")));
    let magnet = attr("magneturl").or_else(|| deep_text(node, "magnetURI"));
    let hash = attr("infohash").or_else(|| deep_text(node, "infoHash"));
    let looks_like_torrent = |link: &str| {
        let lower = link.to_ascii_lowercase();
        lower.starts_with("magnet:") || lower.split('?').next().unwrap_or("").ends_with(".torrent")
    };
    let download = magnet
        .or_else(|| enclosure_url.map(String::from))
        .or_else(|| {
            link_text
                .filter(|link| looks_like_torrent(link))
                .map(String::from)
        })
        .or_else(|| hash.map(|hash| format!("magnet:?xt=urn:btih:{hash}")))
        .or_else(|| link_text.map(String::from))
        .unwrap_or_default();
    let guid = text(node, "guid");
    let page = text(node, "comments")
        .or_else(|| href(Some("alternate")).or(href(None)).map(String::from))
        .or_else(|| link_text.map(String::from))
        .or_else(|| guid.clone())
        .filter(|page| page.starts_with("http") && *page != download);
    let id = guid
        .or_else(|| text(node, "id"))
        .or_else(|| (!download.is_empty()).then(|| download.clone()))
        .unwrap_or_else(|| title.clone());
    if id.is_empty() {
        return None;
    }
    let published = ["pubDate", "published", "updated", "date"]
        .into_iter()
        .find_map(|name| text(node, name).as_deref().and_then(date));
    let size = enclosure
        .and_then(|enclosure| enclosure.attribute("length"))
        .and_then(|length| length.trim().parse().ok())
        .or_else(|| attr("size").and_then(|size| size.parse().ok()))
        .or_else(|| deep_text(node, "contentLength").and_then(|size| size.parse().ok()))
        .or_else(|| text(node, "size").as_deref().and_then(human_size))
        .filter(|size| *size > 0);
    Some(FeedItem {
        id,
        title,
        link: download,
        page,
        published,
        size,
        rule: None,
    })
}

fn child<'a, 'input>(node: Node<'a, 'input>, name: &str) -> Option<Node<'a, 'input>> {
    node.children()
        .find(|child| child.is_element() && child.tag_name().name() == name)
}

fn text(node: Node, name: &str) -> Option<String> {
    child(node, name)?
        .text()
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(String::from)
}

fn deep_text(node: Node, name: &str) -> Option<String> {
    node.descendants()
        .find(|child| child.is_element() && child.tag_name().name() == name)?
        .text()
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(String::from)
}

fn date(text: &str) -> Option<i64> {
    chrono::DateTime::parse_from_rfc2822(text)
        .or_else(|_| chrono::DateTime::parse_from_rfc3339(text))
        .map(|at| at.timestamp())
        .ok()
}

/// Sizes such as "1.4 GiB" or "700 MB", as some sites write them.
fn human_size(text: &str) -> Option<u64> {
    let text = text.trim();
    let split = text
        .find(|c: char| !(c.is_ascii_digit() || c == '.'))
        .unwrap_or(text.len());
    let number: f64 = text[..split].parse().ok()?;
    let unit = text[split..].trim().to_ascii_lowercase();
    let scale: f64 = match unit.as_str() {
        "" | "b" | "bytes" => 1.0,
        "kb" | "kib" => 1024.0,
        "mb" | "mib" => 1024.0 * 1024.0,
        "gb" | "gib" => 1024.0 * 1024.0 * 1024.0,
        "tb" | "tib" => 1024.0 * 1024.0 * 1024.0 * 1024.0,
        _ => return None,
    };
    Some((number * scale) as u64)
}

/// A rule's word lists, ready to test titles.
pub struct Matcher {
    must: Vec<Vec<Regex>>,
    must_not: Vec<Vec<Regex>>,
}

impl Matcher {
    pub fn new(rule: &Rule) -> Result<Self> {
        Ok(Self {
            must: words(&rule.must_contain, rule.regex)
                .map_err(|err| anyhow!("must contain: {err}"))?,
            must_not: words(&rule.must_not_contain, rule.regex)
                .map_err(|err| anyhow!("must not contain: {err}"))?,
        })
    }

    pub fn matches(&self, title: &str) -> bool {
        let hits = |groups: &[Vec<Regex>]| {
            groups
                .iter()
                .any(|group| group.iter().all(|word| word.is_match(title)))
        };
        (self.must.is_empty() || hits(&self.must)) && !hits(&self.must_not)
    }
}

/// Alternatives split on `|`, each a set of words that must all appear.
fn words(text: &str, regex: bool) -> Result<Vec<Vec<Regex>>> {
    let text = text.trim();
    if text.is_empty() {
        return Ok(Vec::new());
    }
    let build = |pattern: &str| {
        RegexBuilder::new(pattern)
            .case_insensitive(true)
            .build()
            .map_err(|_| anyhow!("{pattern} is not a valid pattern"))
    };
    if regex {
        return Ok(vec![vec![build(text)?]]);
    }
    text.split('|')
        .map(|group| {
            group
                .split_whitespace()
                .map(|word| {
                    let pattern = regex::escape(word).replace(r"\*", ".*").replace(r"\?", ".");
                    build(&pattern)
                })
                .collect::<Result<Vec<_>>>()
        })
        .filter(|group| group.as_ref().is_ok_and(|words| !words.is_empty()) || group.is_err())
        .collect()
}

static EPISODE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\bS(\d{1,4})[ ._-]?E(\d{1,4})(?:[ ._-]?-?E(\d{1,4}))?").expect("valid")
});
static CROSS: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\b(\d{1,2})x(\d{1,3})\b").expect("valid"));
static DAILY: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\b((?:19|20)\d{2})[ ._-](\d{2})[ ._-](\d{2})\b").expect("valid"));
static FIX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\b(repack|proper)\b").expect("valid"));

/// The episode a title is, such as s1e2, so the same episode in another quality is known.
pub fn episode(title: &str) -> Option<String> {
    let number = |text: Option<regex::Match>| text?.as_str().parse::<u32>().ok();
    let key = if let Some(found) = EPISODE.captures(title) {
        let (season, first) = (number(found.get(1))?, number(found.get(2))?);
        match number(found.get(3)) {
            Some(last) if last > first => format!("s{season}e{first}-{last}"),
            _ => format!("s{season}e{first}"),
        }
    } else if let Some(found) = CROSS.captures(title) {
        format!("s{}e{}", number(found.get(1))?, number(found.get(2))?)
    } else {
        let found = DAILY.captures(title)?;
        format!("{}-{}-{}", &found[1], &found[2], &found[3])
    };
    Some(if FIX.is_match(title) {
        format!("{key} fix")
    } else {
        key
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const RSS: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<rss version="2.0" xmlns:torznab="http://torznab.com/schemas/2015/feed">
  <channel>
    <title>Tracker</title>
    <item>
      <title>Show S01E02 1080p</title>
      <guid isPermaLink="false">a2</guid>
      <link>https://tracker.example/details/2</link>
      <pubDate>Tue, 03 Nov 2026 10:00:00 +0000</pubDate>
      <enclosure url="https://tracker.example/download/2.torrent" length="1000" type="application/x-bittorrent" />
    </item>
    <item>
      <title>Show S01E01 720p</title>
      <guid>a1</guid>
      <torznab:attr name="magneturl" value="magnet:?xt=urn:btih:abc" />
      <torznab:attr name="size" value="2000" />
    </item>
  </channel>
</rss>"#;

    const ATOM: &str = r#"<feed xmlns="http://www.w3.org/2005/Atom">
  <title>Atom feed</title>
  <entry>
    <title>Movie 2026</title>
    <id>urn:1</id>
    <updated>2026-11-03T10:00:00Z</updated>
    <link rel="alternate" href="https://site.example/movie" />
    <link rel="enclosure" href="https://site.example/movie.torrent" />
  </entry>
</feed>"#;

    #[test]
    fn reads_rss_with_torznab_extras() {
        let (title, items) = parse(RSS).unwrap();
        assert_eq!(title, "Tracker");
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].id, "a2");
        assert_eq!(items[0].link, "https://tracker.example/download/2.torrent");
        assert_eq!(
            items[0].page.as_deref(),
            Some("https://tracker.example/details/2")
        );
        assert_eq!(items[0].size, Some(1000));
        assert_eq!(items[0].published, Some(1793700000));
        assert_eq!(items[1].link, "magnet:?xt=urn:btih:abc");
        assert_eq!(items[1].size, Some(2000));
    }

    #[test]
    fn reads_atom() {
        let (title, items) = parse(ATOM).unwrap();
        assert_eq!(title, "Atom feed");
        assert_eq!(items[0].link, "https://site.example/movie.torrent");
        assert_eq!(items[0].page.as_deref(), Some("https://site.example/movie"));
        assert!(items[0].published.is_some());
    }

    #[test]
    fn web_pages_are_not_feeds() {
        assert!(parse("<html><body>hi</body></html>").is_err());
        assert!(parse("not xml").is_err());
    }

    fn rule(must: &str, not: &str, regex: bool) -> Rule {
        Rule {
            name: "r".into(),
            must_contain: must.into(),
            must_not_contain: not.into(),
            regex,
            ..Rule::default()
        }
    }

    #[test]
    fn words_match_anywhere_and_alternatives_split_on_bars() {
        let matcher = Matcher::new(&rule("show 1080p | other*720p", "cam", false)).unwrap();
        assert!(matcher.matches("The.Show.S01E02.1080p.WEB"));
        assert!(matcher.matches("Other Thing 720p"));
        assert!(!matcher.matches("The Show S01E02 720p"));
        assert!(!matcher.matches("The Show 1080p CAM"));
        assert!(
            Matcher::new(&rule("", "", false))
                .unwrap()
                .matches("anything")
        );
    }

    #[test]
    fn regex_rules_match_and_bad_patterns_are_refused() {
        let matcher = Matcher::new(&rule(r"show\.s\d+e\d+", "", true)).unwrap();
        assert!(matcher.matches("Show.S01E02.mkv"));
        assert!(Matcher::new(&rule("(", "", true)).is_err());
    }

    #[test]
    fn episodes_are_recognized_in_the_usual_spellings() {
        assert_eq!(episode("Show.S01E02.1080p").as_deref(), Some("s1e2"));
        assert_eq!(episode("Show S01E02E03").as_deref(), Some("s1e2-3"));
        assert_eq!(episode("Show 1x02 720p").as_deref(), Some("s1e2"));
        assert_eq!(episode("News 2026.11.03").as_deref(), Some("2026-11-03"));
        assert_eq!(episode("Show S01E02 REPACK").as_deref(), Some("s1e2 fix"));
        assert_eq!(episode("Movie 1080p"), None);
    }

    #[tokio::test]
    #[ignore = "reads live feeds over the network"]
    async fn reads_live_torrent_feeds() {
        for url in [
            "https://distrowatch.com/news/torrents.xml",
            "https://academictorrents.com/collection/datasets.rss",
        ] {
            let (title, items) = fetch(&client(), url).await.unwrap();
            assert!(!title.is_empty(), "{url} has a title");
            assert!(!items.is_empty(), "{url} lists items");
            for item in &items {
                assert!(!item.title.is_empty(), "{url}: {item:?}");
                let link = item.link.to_ascii_lowercase();
                assert!(
                    link.starts_with("magnet:") || link.starts_with("http"),
                    "{url}: {item:?}"
                );
            }
            println!(
                "{url}: {title}, {} items, first {:?}",
                items.len(),
                items[0]
            );
        }
    }

    #[test]
    fn sizes_in_words() {
        assert_eq!(human_size("1.5 GiB"), Some(1610612736));
        assert_eq!(human_size("700 MB"), Some(734003200));
        assert_eq!(human_size("big"), None);
    }

    #[test]
    fn rules_take_new_items_once_per_episode_and_the_first_check_takes_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let feeds = Feeds::load(dir.path().join("rss.json"));
        let id = feeds.add_feed("https://feed.example/rss").unwrap();
        feeds
            .save_rule(Rule {
                name: "show".into(),
                must_contain: "show".into(),
                smart_episodes: true,
                category: Some("tv".into()),
                ..Rule::default()
            })
            .unwrap();
        let apply = |xml: &str| feeds.apply(&id, parse(xml));
        assert!(apply(RSS).is_empty());
        let newer = RSS
            .replace("<guid isPermaLink=\"false\">a2</guid>", "<guid>a3</guid>")
            .replace("S01E02 1080p", "S01E03 1080p")
            .replace("<guid>a1</guid>", "<guid>a4</guid>")
            .replace("S01E01 720p", "S01E03 720p");
        let takes = apply(&newer);
        assert_eq!(takes.len(), 1);
        assert_eq!(takes[0].category.as_deref(), Some("tv"));
        assert!(takes[0].title.contains("720p"));
        assert!(apply(&newer).is_empty());
        let rss = feeds.get();
        assert_eq!(rss.feeds[0].items.len(), 4);
        assert_eq!(rss.rules[0].episodes, ["s1e3"]);
        let stored = Feeds::load(dir.path().join("rss.json")).get();
        assert_eq!(stored, rss);
    }
}
