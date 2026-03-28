use scraper::{Html, Selector};
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

const BASE_URL: &str = "https://nyaa.si";

// Shared HTTP client (created once, reused for all requests)
static CLIENT: OnceLock<reqwest::blocking::Client> = OnceLock::new();

fn client() -> &'static reqwest::blocking::Client {
    CLIENT.get_or_init(|| {
        use reqwest::header::{self, HeaderMap, HeaderValue};

        let mut hdrs = HeaderMap::new();
        hdrs.insert(
            header::ACCEPT,
            HeaderValue::from_static(
                "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8",
            ),
        );
        hdrs.insert(header::ACCEPT_LANGUAGE, HeaderValue::from_static("en-US,en;q=0.5"));
        hdrs.insert(header::CACHE_CONTROL, HeaderValue::from_static("max-age=0"));
        hdrs.insert("Upgrade-Insecure-Requests", HeaderValue::from_static("1"));

        reqwest::blocking::Client::builder()
            .user_agent(
                "Mozilla/5.0 (Windows NT 10.0; Win64; x64) \
                 AppleWebKit/537.36 (KHTML, like Gecko) \
                 Chrome/124.0.0.0 Safari/537.36",
            )
            .default_headers(hdrs)
            .cookie_store(true)
            .timeout(std::time::Duration::from_secs(20))
            .build()
            .expect("failed to build HTTP client")
    })
}

// Icon cache
static ICON_CACHE: OnceLock<Mutex<HashMap<String, Vec<u8>>>> = OnceLock::new();

fn icon_cache() -> &'static Mutex<HashMap<String, Vec<u8>>> {
    ICON_CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

pub fn fetch_icon_bytes(code: &str) -> Option<Vec<u8>> {
    {
        if let Ok(cache) = icon_cache().lock() {
            if let Some(bytes) = cache.get(code) {
                return Some(bytes.clone());
            }
        }
    }
    let url = format!("{}/static/img/icons/nyaa/{}.png", BASE_URL, code);
    eprintln!("[icon] GET {}", url);
    let bytes = client().get(&url).send().ok()?.bytes().ok()?.to_vec();
    if let Ok(mut cache) = icon_cache().lock() {
        cache.insert(code.to_string(), bytes.clone());
    }
    Some(bytes)
}

// Search
#[derive(Debug, Clone)]
pub struct TorrentEntry {
    pub category: String,
    pub category_code: String,
    pub name: String,
    pub view_link: String,
    pub download_link: String,
    pub magnet_link: String,
    pub size: String,
    pub date: String,
    pub seeders: String,
    pub leechers: String,
    pub completed: String,
    pub row_type: i32,
}

#[derive(Debug)]
pub struct SearchResult {
    pub entries: Vec<TorrentEntry>,
    pub current_page: i32,
    pub total_pages: i32,
    pub page_list: Vec<i32>, // 0 = ellipsis "..."
}

pub fn search(query: &str, page: i32) -> Result<SearchResult, reqwest::Error> {
    let client = client();

    let mut params: Vec<(&str, String)> = vec![("p", page.to_string())];
    if !query.is_empty() {
        params.push(("q", query.to_string()));
    }

    eprintln!("[nyaa] GET {}  query={:?}  page={}", BASE_URL, query, page);
    let response = client.get(BASE_URL).query(&params).send()?;
    let status = response.status();
    eprintln!("[nyaa] HTTP {}", status);
    let bytes = response.bytes()?;
    let html = String::from_utf8_lossy(&bytes).into_owned();
    eprintln!("[nyaa] body len = {} bytes", html.len());

    let result = parse_list(&html);
    eprintln!(
        "[nyaa] parsed {} entries, page {}/{}",
        result.entries.len(),
        result.current_page,
        result.total_pages
    );
    Ok(result)
}

fn parse_list(html: &str) -> SearchResult {
    let document = Html::parse_document(html);

    let row_sel = Selector::parse("table.torrent-list tbody tr").unwrap();
    let td_sel = Selector::parse("td").unwrap();
    let a_sel = Selector::parse("a").unwrap();

    let mut entries = Vec::new();

    for row in document.select(&row_sel) {
        let row_type = match row.value().attr("class").unwrap_or("default") {
            c if c.contains("success") => 1,
            c if c.contains("danger") => 2,
            c if c.contains("warning") => 3,
            _ => 0,
        };

        let tds: Vec<_> = row.select(&td_sel).collect();
        if tds.len() < 8 {
            continue;
        }

        // td[0] : category
        let cat_a = tds[0].select(&a_sel).next();
        let category = cat_a
            .and_then(|a| a.value().attr("title"))
            .unwrap_or("")
            .to_string();
        let category_code = cat_a
            .and_then(|a| a.value().attr("href"))
            .and_then(|h| h.strip_prefix("/?c="))
            .unwrap_or("0_0")
            .to_string();

        // td[1] (colspan=2) : name link - use last <a> to skip optional comments link
        let name_a = tds[1].select(&a_sel).last();
        let name = name_a
            .map(|a| {
                a.value()
                    .attr("title")
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| a.text().collect::<String>().trim().to_string())
            })
            .unwrap_or_default();
        let view_link = name_a
            .and_then(|a| a.value().attr("href"))
            .map(|h| format!("{}{}", BASE_URL, h))
            .unwrap_or_default();

        // td[2] : download / magnet
        let links: Vec<_> = tds[2].select(&a_sel).collect();
        let download_link = links
            .iter()
            .find(|a| a.value().attr("href").map_or(false, |h| h.contains("/download/")))
            .and_then(|a| a.value().attr("href"))
            .map(|h| format!("{}{}", BASE_URL, h))
            .unwrap_or_default();
        let magnet_link = links
            .iter()
            .find(|a| a.value().attr("href").map_or(false, |h| h.starts_with("magnet:")))
            .and_then(|a| a.value().attr("href"))
            .unwrap_or("")
            .to_string();

        let size = tds[3].text().collect::<String>().trim().to_string();
        let date = tds[4].text().collect::<String>().trim().to_string();
        let seeders = tds[5].text().collect::<String>().trim().to_string();
        let leechers = tds[6].text().collect::<String>().trim().to_string();
        let completed = tds[7].text().collect::<String>().trim().to_string();

        entries.push(TorrentEntry {
            category, category_code, name, view_link, download_link, magnet_link,
            size, date, seeders, leechers, completed, row_type,
        });
    }

    // Pagination - parse actual items from HTML (preserving ellipsis positions)
    let active_sel = Selector::parse("ul.pagination li.active a").unwrap();
    let page_li_sel = Selector::parse("ul.pagination li").unwrap();

    let current_page = document
        .select(&active_sel)
        .next()
        .and_then(|a| {
            a.text()
                .collect::<String>()
                .split_whitespace()
                .next()
                .and_then(|s| s.parse::<i32>().ok())
        })
        .unwrap_or(1);

    let mut page_list: Vec<i32> = Vec::new();
    let mut total_pages = current_page;

    for li in document.select(&page_li_sel) {
        let a = match li.select(&a_sel).next() {
            Some(a) => a,
            None => continue,
        };
        // Skip prev / next arrows
        if a.value().attr("rel").is_some() { continue; }

        let class = li.value().attr("class").unwrap_or("");
        // Ellipsis item (disabled "…") - disabled "«"/"»" arrows are NOT ellipsis
        if class.contains("disabled") {
            let text = a.text().collect::<String>();
            let trimmed = text.trim();
            if trimmed == "\u{2026}" || trimmed == "..." {
                if page_list.last() != Some(&0) {
                    page_list.push(0);
                }
            }
            continue;
        }
        let text = a.text().collect::<String>();
        if let Ok(n) = text.split_whitespace().next().unwrap_or("").parse::<i32>() {
            page_list.push(n);
            if n > total_pages { total_pages = n; }
        }
    }

    SearchResult { entries, current_page, total_pages, page_list }
}

// Detail page
#[derive(Debug, Clone)]
pub struct DetailData {
    pub title: String,
    pub category: String,
    pub date: String,
    pub submitter: String,
    pub seeders: String,
    pub leechers: String,
    pub file_size: String,
    pub completed: String,
    pub info_hash: String,
    pub download_link: String,
    pub magnet_link: String,
}

pub fn get_detail(view_url: &str) -> Result<DetailData, reqwest::Error> {
    let client = client();
    eprintln!("[detail] GET {}", view_url);
    let response = client.get(view_url).send()?;
    eprintln!("[detail] HTTP {}", response.status());
    let bytes = response.bytes()?;
    let html = String::from_utf8_lossy(&bytes).into_owned();
    Ok(parse_detail(&html, view_url))
}

fn parse_detail(html: &str, _view_url: &str) -> DetailData {
    let document = Html::parse_document(html);

    // Title
    let title_sel = Selector::parse(".panel-title").unwrap();
    let title = document
        .select(&title_sel)
        .next()
        .map(|e| e.text().collect::<String>().trim().to_string())
        .unwrap_or_default();

    // Parse label > value pairs from panel-body rows
    let row_sel = Selector::parse(".panel-body .row").unwrap();
    let mut fields: HashMap<String, String> = HashMap::new();

    for row in document.select(&row_sel) {
        // Collect direct element children of the row
        let children: Vec<_> = row
            .children()
            .filter_map(scraper::ElementRef::wrap)
            .collect();

        let mut i = 0;
        while i + 1 < children.len() {
            let lc = children[i].value().attr("class").unwrap_or("");
            let vc = children[i + 1].value().attr("class").unwrap_or("");
            if lc.contains("col-md-1") && vc.contains("col-md-5") {
                let label = children[i]
                    .text()
                    .collect::<String>()
                    .trim()
                    .trim_end_matches(':')
                    .to_lowercase();
                let value = children[i + 1].text().collect::<String>().trim().to_string();
                fields.insert(label, value);
                i += 2;
            } else {
                i += 1;
            }
        }
    }

    // Info hash from <kbd>
    let kbd_sel = Selector::parse("kbd").unwrap();
    let info_hash = document
        .select(&kbd_sel)
        .next()
        .map(|e| e.text().collect::<String>().trim().to_string())
        .unwrap_or_default();

    // Footer links
    let footer_a_sel = Selector::parse(".panel-footer a").unwrap();
    let footer_links: Vec<_> = document.select(&footer_a_sel).collect();

    let download_link = footer_links
        .iter()
        .find(|a| a.value().attr("href").map_or(false, |h| h.contains("/download/")))
        .and_then(|a| a.value().attr("href"))
        .map(|h| {
            if h.starts_with('/') {
                format!("{}{}", BASE_URL, h)
            } else {
                h.to_string()
            }
        })
        .unwrap_or_default();

    let magnet_link = footer_links
        .iter()
        .find(|a| a.value().attr("href").map_or(false, |h| h.starts_with("magnet:")))
        .and_then(|a| a.value().attr("href"))
        .unwrap_or("")
        .to_string();

    // Category : join all link texts in that value cell
    let category_sel = Selector::parse(".panel-body .row:first-child .col-md-5").unwrap();
    let category = document
        .select(&category_sel)
        .next()
        .map(|e| e.text().collect::<Vec<_>>().join(" ").split_whitespace().collect::<Vec<_>>().join(" "))
        .unwrap_or_else(|| fields.get("category").cloned().unwrap_or_default());

    // Submitter link text
    let sub_sel = Selector::parse(".panel-body .row a[href*='/user/']").unwrap();
    let submitter = document
        .select(&sub_sel)
        .next()
        .map(|e| e.text().collect::<String>().trim().to_string())
        .unwrap_or_else(|| fields.get("submitter").cloned().unwrap_or_default());

    DetailData {
        title,
        category,
        date: fields.get("date").cloned().unwrap_or_else(|| {
            // fallback : timestamp attr
            let ts_sel = Selector::parse("[data-timestamp]").unwrap();
            document
                .select(&ts_sel)
                .next()
                .map(|e| e.text().collect::<String>().trim().to_string())
                .unwrap_or_default()
        }),
        submitter,
        seeders: fields.get("seeders").cloned().unwrap_or_default(),
        leechers: fields.get("leechers").cloned().unwrap_or_default(),
        file_size: fields.get("file size").cloned().unwrap_or_default(),
        completed: fields.get("completed").cloned().unwrap_or_default(),
        info_hash,
        download_link,
        magnet_link,
    }
}

// Batch download .torrent files 
fn sanitize_filename(name: &str) -> String {
    name.chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            c => c,
        })
        .collect::<String>()
        .trim()
        .to_string()
}

/// `entries` is a slice of (title, download_url) pairs.
/// `on_progress(index, total, title, is_done)` is called before (is_done=false)
/// and after (is_done=true) each individual download.
pub fn download_torrents(
    entries: &[(String, String)],
    save_path: &str,
    on_progress: impl Fn(usize, usize, &str, bool),
) -> Result<usize, String> {
    let path = std::path::Path::new(save_path);
    std::fs::create_dir_all(path).map_err(|e| e.to_string())?;

    let client = client();
    let mut count = 0;

    for (idx, (title, link)) in entries.iter().enumerate() {
        on_progress(idx, entries.len(), title, false);
        let safe = sanitize_filename(title);
        let filename = if safe.is_empty() {
            format!("{}.torrent", link.split('/').last().unwrap_or("file"))
        } else {
            format!("{}.torrent", safe)
        };
        let dest = path.join(&filename);
        let bytes = client
            .get(link)
            .send()
            .and_then(|r| r.bytes())
            .map_err(|e| e.to_string())?;
        std::fs::write(&dest, &bytes).map_err(|e| e.to_string())?;
        eprintln!("[download] saved {}", dest.display());
        on_progress(idx, entries.len(), title, true);
        count += 1;
    }

    Ok(count)
}
