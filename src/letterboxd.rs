use crate::wallerror::WallError;
use std::sync::LazyLock;

/// Matches the `<img src="...">` inside a diary entry's `<description>` HTML
/// — same target as the website's `$('p img').attr('src')` (cheerio), just
/// without pulling in a full HTML parser for one attribute.
static IMG_SRC_RE: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r#"<img[^>]*\ssrc="([^"]*)""#).unwrap());

/// Normalizes whatever crop size Letterboxd embedded in the poster URL to
/// the canonical 230x345 crop, matching `letterboxd.ts`'s `getPoster()`.
/// `poster_layout::sized_poster_url` rewrites this again at fetch time to
/// whichever CDN-safe height the layout actually needs.
static CROP_RE: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r"-0-.*-crop").unwrap());

fn extract_poster(description: &str) -> Option<String> {
    let src = IMG_SRC_RE.captures(description)?.get(1)?.as_str();
    Some(CROP_RE.replace(src, "-0-230-0-345-crop").into_owned())
}

/// Concatenates every text/CDATA descendant of a node — RSS feeds mix plain
/// text and CDATA sections, and roxmltree only merges them under `.text()`
/// when there's exactly one child.
fn node_text(node: roxmltree::Node) -> String {
    node.descendants()
        .filter(|n| n.is_text())
        .filter_map(|n| n.text())
        .collect()
}

fn child_text<'a>(item: &roxmltree::Node<'a, 'a>, tag: &str) -> String {
    item.children()
        .find(|n| n.has_tag_name(tag))
        .map(node_text)
        .unwrap_or_default()
}

/// Letterboxd usernames are alphanumeric/underscore/hyphen, but percent-encode
/// defensively anyway since this goes straight into a URL path segment.
fn urlencode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
            out.push(c);
        } else {
            for b in c.to_string().as_bytes() {
                out.push_str(&format!("%{b:02X}"));
            }
        }
    }
    out
}

/// Fetches a user's public Letterboxd diary RSS feed and returns each
/// entry's poster URL in feed order (most recent first), `None` where an
/// entry has no poster. List items (as opposed to diary/film entries) are
/// dropped entirely, matching `letterboxd.ts`'s `fetchDiary()`.
pub fn fetch_diary_posters(
    username: &str,
    client: &reqwest::blocking::Client,
) -> Result<Vec<Option<String>>, WallError> {
    let url = format!("https://letterboxd.com/{}/rss/", urlencode(username));
    let resp = client.get(&url).send()?;
    if resp.status() == reqwest::StatusCode::NOT_FOUND {
        return Err(WallError::NotFound(format!(
            "Letterboxd username '{username}' not found"
        )));
    }
    let resp = resp.error_for_status()?;
    let xml = resp.text()?;

    let doc = roxmltree::Document::parse(&xml)
        .map_err(|e| WallError::Other(format!("bad RSS feed: {e}")))?;

    let mut posters = Vec::new();
    for item in doc.descendants().filter(|n| n.has_tag_name("item")) {
        let link = child_text(&item, "link");
        if link.contains("/list/") {
            continue;
        }
        let description = child_text(&item, "description");
        posters.push(extract_poster(&description));
    }
    Ok(posters)
}
