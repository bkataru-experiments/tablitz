//! Search, deduplication, and AI categorization features for tablitz.
//!
//! This crate provides:
//! - Fuzzy matching on titles and URLs using nucleo
//! - Title normalization with Unicode NFC
//! - URL deduplication with multiple strategies
//! - Full-text search index (optional feature)
//! - Semantic search with embeddings (optional feature)
//! - Auto-categorization for tab groups (optional feature)

use tablitz_core::{Tab, TabGroup, TabSession};
use std::collections::HashSet;
use nucleo::{Matcher, Config};
use nucleo::pattern::{Pattern, CaseMatching, Normalization};
use url::Url;

/// A search result with rank information.
#[derive(Debug, Clone)]
pub struct SearchResult {
    /// The matched tab.
    pub tab: Tab,
    /// The group ID that contains this tab.
    pub group_id: String,
    /// Match score (higher = better match).
    pub score: f32,
    /// The type of search that produced this result.
    pub match_kind: MatchKind,
}

/// The type of search technique used to produce a result.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum MatchKind {
    /// Fuzzy matching on title/URL.
    Fuzzy,
    /// Full-text keyword search.
    FullText,
    /// Semantic embedding similarity.
    Semantic,
    /// Exact string match.
    Exact,
}

/// Fuzzy search using nucleo matcher.
pub struct FuzzySearcher;

impl FuzzySearcher {
    /// Search across all tabs in a session, matching against both title and URL.
    ///
    /// Returns results sorted by score descending.
    pub fn search(query: &str, session: &TabSession) -> Vec<SearchResult> {
        let mut results = Vec::new();
        let mut matcher = Matcher::new(Config::DEFAULT);
        let pattern = Pattern::parse(query, CaseMatching::Smart, Normalization::Smart);
        let mut buf = Vec::new();

        for group in &session.groups {
            let group_id = group.id.clone();
            for tab in &group.tabs {
                if let Some(score) = Self::score_tab(&pattern, tab, &mut matcher, &mut buf) {
                    results.push(SearchResult {
                        tab: tab.clone(),
                        group_id: group_id.clone(),
                        score,
                        match_kind: MatchKind::Fuzzy,
                    });
                }
            }
        }

        results.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap());
        results
    }

    /// Search only tab titles.
    pub fn search_titles(query: &str, session: &TabSession) -> Vec<SearchResult> {
        let mut results = Vec::new();
        let mut matcher = Matcher::new(Config::DEFAULT);
        let pattern = Pattern::parse(query, CaseMatching::Smart, Normalization::Smart);
        let mut buf = Vec::new();

        for group in &session.groups {
            let group_id = group.id.clone();
            for tab in &group.tabs {
                let haystack = nucleo::Utf32Str::new(&tab.title, &mut buf);
                if let Some(score) = pattern.score(haystack, &mut matcher) {
                    results.push(SearchResult {
                        tab: tab.clone(),
                        group_id: group_id.clone(),
                        score: score as f32,
                        match_kind: MatchKind::Fuzzy,
                    });
                    buf.clear();
                }
            }
        }

        results.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap());
        results
    }

    /// Search only tab URLs.
    pub fn search_urls(query: &str, session: &TabSession) -> Vec<SearchResult> {
        let mut results = Vec::new();
        let mut matcher = Matcher::new(Config::DEFAULT);
        let pattern = Pattern::parse(query, CaseMatching::Smart, Normalization::Smart);
        let mut buf = Vec::new();

        for group in &session.groups {
            let group_id = group.id.clone();
            for tab in &group.tabs {
                let haystack = nucleo::Utf32Str::new(tab.url.as_str(), &mut buf);
                if let Some(score) = pattern.score(haystack, &mut matcher) {
                    results.push(SearchResult {
                        tab: tab.clone(),
                        group_id: group_id.clone(),
                        score: score as f32,
                        match_kind: MatchKind::Fuzzy,
                    });
                    buf.clear();
                }
            }
        }

        results.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap());
        results
    }

    /// Score a tab by matching against both title and URL, taking the max.
    fn score_tab(
        pattern: &Pattern,
        tab: &Tab,
        matcher: &mut Matcher,
        buf: &mut Vec<char>,
    ) -> Option<f32> {
        let max_score = Self::score_text(pattern, &tab.title, matcher, buf)
            .max(Self::score_text(pattern, tab.url.as_str(), matcher, buf));

        max_score.map(|s| s as f32)
    }

    /// Score a single text string.
    fn score_text(
        pattern: &Pattern,
        text: &str,
        matcher: &mut Matcher,
        buf: &mut Vec<char>,
    ) -> Option<u32> {
        let haystack = nucleo::Utf32Str::new(text, buf);
        let score = pattern.score(haystack, matcher);
        buf.clear();
        score
    }
}

/// Normalizes tab titles for improved matching.
pub struct TitleNormalizer;

impl TitleNormalizer {
    /// Common site suffixes to strip from titles.
    const SUFFIXES: &[&str] = &[
        " - Google Search",
        " | Twitter",
        " on X",
        " - YouTube",
        " on YouTube",
        " - Wikipedia",
        " - Reddit",
        " | LinkedIn",
        " - Stack Overflow",
        " | GitHub",
        " | daily.dev",
        " | DEV Community",
        " | Hacker News",
        " | Medium",
        " – Frontend Masters Blog",
        " | InfoWorld",
        " | Product Hunt",
    ];

    /// Normalize a single title.
    ///
    /// Applies Unicode NFC normalization, trims whitespace,
    /// collapses multiple spaces, and strips common site suffixes.
    pub fn normalize(title: &str) -> String {
        use unicode_normalization::UnicodeNormalization;

        // 1. Unicode NFC normalization
        let normalized = title.nfc().collect::<String>();

        // 2. Trim whitespace
        let trimmed = normalized.trim();

        // 3. Collapse multiple spaces to single
        let collapsed = trimmed.split_whitespace().collect::<Vec<_>>().join(" ");

        // 4. Strip common site suffixes (from last occurrence)
        let mut result = collapsed;
        for suffix in Self::SUFFIXES {
            if let Some(pos) = result.rfind(suffix) {
                result = result[..pos].to_string();
            }
        }

        result.trim().to_string()
    }

    /// Normalize all titles in a session.
    pub fn normalize_session(session: &TabSession) -> TabSession {
        let mut normalized = session.clone();
        for group in &mut normalized.groups {
            for tab in &mut group.tabs {
                tab.title = Self::normalize(&tab.title);
            }
        }
        normalized
    }
}

/// Deduplicates tabs based on URL similarity.
pub struct DedupEngine;

/// Strategy for determining duplicate URLs.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DedupStrategy {
    /// Exact URL match.
    ExactUrl,
    /// Normalized URL match (case-insensitive, query params normalized).
    NormalizedUrl,
    /// Fuzzy match on URLs.
    FuzzyUrl { threshold: f32 },
    /// Match on both URL and title.
    UrlAndTitle,
}

/// Result of a deduplication operation.
#[derive(Debug)]
pub struct DedupResult {
    /// Original number of tabs before deduplication.
    pub original_count: usize,
    /// Number of tabs after deduplication.
    pub deduplicated_count: usize,
    /// Tabs that were removed as duplicates.
    pub removed: Vec<Tab>,
    /// The deduplicated session.
    pub session: TabSession,
}

impl DedupEngine {
    /// Deduplicate tabs in a session using the specified strategy.
    pub fn dedup(session: &TabSession, strategy: DedupStrategy) -> DedupResult {
        match strategy {
            DedupStrategy::ExactUrl => Self::dedup_exact_url(session),
            DedupStrategy::NormalizedUrl => Self::dedup_normalized_url(session),
            DedupStrategy::FuzzyUrl { threshold } => Self::dedup_fuzzy_url(session, threshold),
            DedupStrategy::UrlAndTitle => Self::dedup_url_and_title(session),
        }
    }

    fn dedup_exact_url(session: &TabSession) -> DedupResult {
        let mut seen_urls = HashSet::new();
        let mut removed = Vec::new();
        let mut deduped_session = session.clone();

        for group in &mut deduped_session.groups {
            let mut kept_tabs = Vec::new();
            for tab in &group.tabs {
                let url_str = tab.url.as_str().to_string();
                if seen_urls.contains(&url_str) {
                    removed.push(tab.clone());
                } else {
                    seen_urls.insert(url_str);
                    kept_tabs.push(tab.clone());
                }
            }
            group.tabs = kept_tabs;
        }

        let original_count = session.total_tab_count();
        let deduplicated_count = deduped_session.total_tab_count();

        DedupResult {
            original_count,
            deduplicated_count,
            removed,
            session: deduped_session,
        }
    }

    fn dedup_normalized_url(session: &TabSession) -> DedupResult {
        let mut seen_urls = HashSet::new();
        let mut removed = Vec::new();
        let mut deduped_session = session.clone();

        for group in &mut deduped_session.groups {
            let mut kept_tabs = Vec::new();
            for tab in &group.tabs {
                let normalized = Self::normalize_url(tab.url.as_str());
                if seen_urls.contains(&normalized) {
                    removed.push(tab.clone());
                } else {
                    seen_urls.insert(normalized);
                    kept_tabs.push(tab.clone());
                }
            }
            group.tabs = kept_tabs;
        }

        let original_count = session.total_tab_count();
        let deduplicated_count = deduped_session.total_tab_count();

        DedupResult {
            original_count,
            deduplicated_count,
            removed,
            session: deduped_session,
        }
    }

    fn dedup_fuzzy_url(session: &TabSession, threshold: f32) -> DedupResult {
        let mut kept_tabs: Vec<(String, Tab)> = Vec::new(); // (normalized_url, tab)
        let mut removed = Vec::new();
        let mut matcher = Matcher::new(Config::DEFAULT);

        for group in &session.groups {
            for tab in &group.tabs {
                let normalized = Self::normalize_url(tab.url.as_str());
                let mut found_duplicate = false;

                for (existing_url, _) in &kept_tabs {
                    if Self::fuzzy_match_urls(&normalized, existing_url, threshold, &mut matcher) {
                        found_duplicate = true;
                        break;
                    }
                }

                if found_duplicate {
                    removed.push(tab.clone());
                } else {
                    kept_tabs.push((normalized, tab.clone()));
                }
            }
        }

        // Rebuild session with kept tabs (grouping gets lost, put all in one group)
        let deduped_session = TabSession {
            version: session.version,
            source: session.source.clone(),
            groups: if kept_tabs.is_empty() {
                vec![]
            } else {
                vec![TabGroup {
                    id: "deduped".to_string(),
                    label: None,
                    created_at: session.created_at,
                    tabs: kept_tabs.into_iter().map(|(_, tab)| tab).collect(),
                    pinned: false,
                    locked: false,
                    starred: false,
                }]
            },
            created_at: session.created_at,
            imported_at: session.imported_at,
        };

        let original_count = session.total_tab_count();
        let deduplicated_count = deduped_session.total_tab_count();

        DedupResult {
            original_count,
            deduplicated_count,
            removed,
            session: deduped_session,
        }
    }

    fn dedup_url_and_title(session: &TabSession) -> DedupResult {
        let mut seen: HashSet<(String, String)> = HashSet::new(); // (normalized_url, normalized_title)
        let mut removed = Vec::new();
        let mut deduped_session = session.clone();

        for group in &mut deduped_session.groups {
            let mut kept_tabs = Vec::new();
            for tab in &group.tabs {
                let normalized_url = Self::normalize_url(tab.url.as_str());
                let normalized_title = TitleNormalizer::normalize(&tab.title);
                let key = (normalized_url, normalized_title);

                if seen.contains(&key) {
                    removed.push(tab.clone());
                } else {
                    seen.insert(key);
                    kept_tabs.push(tab.clone());
                }
            }
            group.tabs = kept_tabs;
        }

        let original_count = session.total_tab_count();
        let deduplicated_count = deduped_session.total_tab_count();

        DedupResult {
            original_count,
            deduplicated_count,
            removed,
            session: deduped_session,
        }
    }

    /// Normalize a URL for comparison.
    ///
    /// Lowercases scheme/host, removes fragment, normalizes query params,
    /// removes trailing slash (unless path is just "/").
    pub fn normalize_url(url: &str) -> String {
        if let Ok(mut parsed) = Url::parse(url) {
            // Lowercase scheme and host
            let scheme: &str = parsed.scheme();
            let _ = parsed.set_scheme(&scheme.to_lowercase());
            if let Some(host_str) = parsed.host_str() {
                let host: String = host_str.to_lowercase();
                let _ = parsed.set_host(Some(&host));
            }

            // Remove fragment
            parsed.set_fragment(None);

            // Remove utm_* query params and sort remaining
            let query_params: Vec<(String, String)> = parsed
                .query_pairs()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .filter(|(k, _)| !k.starts_with("utm_"))
                .collect();

            if query_params.is_empty() {
                parsed.set_query(None);
            } else {
                let mut sorted: Vec<(String, String)> = query_params;
                sorted.sort_by(|a, b| a.0.cmp(&b.0));
                let query_string = sorted
                    .iter()
                    .map(|(k, v)| format!("{}={}", k, v))
                    .collect::<Vec<_>>()
                    .join("&");
                parsed.set_query(Some(&query_string));
            }

            // Remove trailing slash from path (unless path is just "/")
            let path: String = parsed.path().to_string();
            if path.ends_with('/') && path.len() > 1 {
                let new_path = &path[..path.len() - 1];
                let _ = parsed.set_path(new_path);
            }

            parsed.to_string()
        } else {
            url.to_lowercase()
        }
    }

    /// Check if two URLs match fuzzily above threshold.
    fn fuzzy_match_urls(a: &str, b: &str, threshold: f32, matcher: &mut Matcher) -> bool {
        let pattern = Pattern::parse(a, CaseMatching::Smart, Normalization::Smart);
        let mut buf = Vec::new();

        let haystack = nucleo::Utf32Str::new(b, &mut buf);
        let score = pattern.score(haystack, matcher);

        match score {
            Some(s) => {
                let normalized = s as f32 / u32::MAX as f32;
                normalized >= threshold
            }
            None => false,
        }
    }
}

#[cfg(feature = "full-text")]
pub use fulltext::FullTextIndex;

#[cfg(feature = "full-text")]
mod fulltext {
    use super::*;
    use anyhow::Context;
    use chrono::{DateTime, Utc};
    use tantivy::{
        schema::{Schema, Field, STORED, TEXT, Value},
        Index, IndexWriter, TantivyDocument, collector::TopDocs, query::QueryParser, ReloadPolicy,
    };

    /// Full-text search index using Tantivy.
    pub struct FullTextIndex {
        index: Index,
        tab_id_field: Field,
        group_id_field: Field,
        title_field: Field,
        url_field: Field,
        favicon_url_field: Field,
        added_at_field: Field,
    }

    impl FullTextIndex {
        /// Build a full-text index from a TabSession.
        pub fn build(session: &TabSession) -> anyhow::Result<Self> {
            let mut schema_builder = Schema::builder();
            let tab_id_field = schema_builder.add_text_field("tab_id", TEXT | STORED);
            let group_id_field = schema_builder.add_text_field("group_id", TEXT | STORED);
            let title_field = schema_builder.add_text_field("title", TEXT | STORED);
            let url_field = schema_builder.add_text_field("url", TEXT | STORED);
            let favicon_url_field = schema_builder.add_text_field("favicon_url", STORED);
            let added_at_field = schema_builder.add_u64_field("added_at", STORED);
            let schema = schema_builder.build();

            let index = Index::create_in_ram(schema);
            let mut writer: IndexWriter = index.writer(50_000_000)?;

            for group in &session.groups {
                for tab in &group.tabs {
                    let mut doc = TantivyDocument::default();
                    doc.add_text(tab_id_field, &tab.id);
                    doc.add_text(group_id_field, &group.id);
                    doc.add_text(title_field, &tab.title);
                    doc.add_text(url_field, tab.url.as_str());
                    if let Some(ref favicon) = tab.favicon_url {
                        doc.add_text(favicon_url_field, favicon);
                    }
                    doc.add_u64(added_at_field, tab.added_at.timestamp_millis() as u64);
                    writer.add_document(doc)?;
                }
            }

            writer.commit()?;

            Ok(FullTextIndex {
                index,
                tab_id_field,
                group_id_field,
                title_field,
                url_field,
                favicon_url_field,
                added_at_field,
            })
        }

        /// Search the full-text index.
        pub fn search(&self, query: &str, limit: usize) -> anyhow::Result<Vec<SearchResult>> {
            let reader = self.index.reader_builder().reload_policy(ReloadPolicy::OnCommitWithDelay).try_into()?;
            let searcher = reader.searcher();

            let query_parser = QueryParser::for_index(&self.index, vec![self.title_field, self.url_field]);
            let query = query_parser.parse_query(query).context("Failed to parse query")?;

            let top_docs = searcher.search(&query, &TopDocs::with_limit(limit))?;

            let mut results = Vec::new();
            for (score, doc_address) in top_docs {
                let retrieved_doc: TantivyDocument = searcher.doc(doc_address)?;

                if let Some(tab_id) = retrieved_doc.get_first(self.tab_id_field).and_then(|v| v.as_str()) {
                    if let Some(title) = retrieved_doc.get_first(self.title_field).and_then(|v| v.as_str()) {
                        if let Some(url_str) = retrieved_doc.get_first(self.url_field).and_then(|v| v.as_str()) {
                            if let Some(group_id) = retrieved_doc.get_first(self.group_id_field).and_then(|v| v.as_str()) {
                                let url = Url::parse(url_str).unwrap_or_else(|_| Url::parse("about:blank").unwrap());
                                let favicon_url = retrieved_doc
                            .get_first(self.favicon_url_field)
                            .and_then(|v| v.as_str())
                            .map(ToString::to_string);
                                let added_ts = retrieved_doc
                            .get_first(self.added_at_field)
                            .and_then(|v| v.as_u64())
                            .unwrap_or(0) as i64;

                                let tab = Tab {
                                    id: tab_id.to_string(),
                                    url,
                                    title: title.to_string(),
                                    favicon_url,
                                    added_at: DateTime::from_timestamp_millis(added_ts)
                                        .unwrap_or_else(Utc::now),
                                };

                                results.push(SearchResult {
                                    tab,
                                    group_id: group_id.to_string(),
                                    score,
                                    match_kind: MatchKind::FullText,
                                });
                            }
                        }
                    }
                }
            }

            Ok(results)
        }
    }
}

#[cfg(feature = "ai")]
pub use semantic::{SemanticIndex, AutoCategorizer};

#[cfg(feature = "ai")]
mod semantic {
    use super::*;
    use anyhow::Context;
    use fastembed::{TextEmbedding, InitOptions, EmbeddingModel};
    use std::collections::{HashMap, HashSet};

    /// Semantic search using vector embeddings.
    pub struct SemanticIndex {
        embeddings: Vec<Vec<f32>>,
        tabs: Vec<(Tab, String)>, // (tab, group_id)
    }

    impl SemanticIndex {
        /// Build a semantic index from a TabSession.
        pub fn build(session: &TabSession) -> anyhow::Result<Self> {
            let model = TextEmbedding::try_new(InitOptions::new(EmbeddingModel::AllMiniLML6V2))
                .context("Failed to initialize embedding model")?;

            // Collect all tabs with their group IDs
            let tabs: Vec<(Tab, String)> = session
                .groups
                .iter()
                .flat_map(|group| {
                    group.tabs.iter().map(|tab| (tab.clone(), group.id.clone()))
                })
                .collect();

            // Prepare texts for embedding (title + URL)
            let texts: Vec<String> = tabs
                .iter()
                .map(|(t, _)| format!("{} {}", t.title, t.url.as_str()))
                .collect();

            let embeddings = model.embed(texts, None)
                .context("Failed to generate embeddings")?;

            Ok(SemanticIndex { embeddings, tabs })
        }

        /// Search the semantic index by query.
        pub fn search(&self, query: &str, limit: usize) -> anyhow::Result<Vec<SearchResult>> {
            let model = TextEmbedding::try_new(InitOptions::new(EmbeddingModel::AllMiniLML6V2))
                .context("Failed to initialize embedding model")?;

            let query_embedding = model.embed(vec![query.to_string()], None)?
                .into_iter()
                .next()
                .context("Failed to embed query")?;

            let mut results: Vec<(f32, usize)> = self
                .embeddings
                .iter()
                .enumerate()
                .map(|(i, emb)| {
                    let score = cosine_similarity(&query_embedding, emb);
                    (score, i)
                })
                .filter(|(score, _)| *score > 0.0)
                .collect();

            results.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap());
            results.truncate(limit);

            let search_results = results
                .into_iter()
                .map(|(score, idx)| {
                    let (tab, group_id) = self.tabs[idx].clone();
                    SearchResult {
                        tab,
                        group_id,
                        score,
                        match_kind: MatchKind::Semantic,
                    }
                })
                .collect();

            Ok(search_results)
        }
    }

    /// Suggest labels for a tab group based on its content.
    pub struct AutoCategorizer;

    impl AutoCategorizer {
        /// Stop words to filter out from titles.
        const STOPWORDS: &[&str] = &[
            "the", "a", "an", "is", "in", "on", "at", "to", "for", "of", "and",
            "or", "with", "from", "by", "as", "but", "not", "this", "that",
            "be", "are", "was", "were", "been", "being", "have", "has", "had",
            "do", "does", "did", "will", "would", "should", "could", "may", "might",
            "must", "can", "all", "each", "every", "both", "few", "more", "most",
            "other", "some", "such", "no", "nor", "only", "own", "same", "so",
            "than", "too", "very", "just", "into", "over", "after", "before",
        ];

        /// Suggest labels for a tab group.
        ///
        /// Returns up to 3 candidate labels with confidence scores:
        /// 1. Most common domain (score 0.8)
        /// 2. Top frequent word (score 0.6)
        /// 3. Second frequent word (score 0.4)
        pub fn suggest_label(group: &TabGroup) -> Vec<(String, f32)> {
            let mut candidates = Vec::new();

            // 1. Extract domains and find most common
            let mut domain_counts: HashMap<&str, usize> = HashMap::new();
            for tab in &group.tabs {
                if let Some(domain) = tab.domain() {
                    *domain_counts.entry(domain).or_insert(0) += 1;
                }
            }

            if let Some((domain, count)) = domain_counts
                .into_iter()
                .max_by_key(|&(_, count)| count) {
                if count > 1 {
                    candidates.push((domain.to_string(), 0.8));
                }
            }

            // 2. Extract words from titles, filter stopwords
            let mut word_counts: HashMap<String, usize> = HashMap::new();
            for tab in &group.tabs {
                for word in tab.title.split_whitespace() {
                    let word = word.to_lowercase();
                    if !Self::STOPWORDS.contains(&word.as_str()) && word.len() > 2 {
                        *word_counts.entry(word).or_insert(0) += 1;
                    }
                }
            }

            // Sort by frequency
            let mut sorted_words: Vec<_> = word_counts.into_iter().collect();
            sorted_words.sort_by(|a, b| b.1.cmp(&a.1));

            // Add top 2 words
            for (i, (word, count)) in sorted_words.into_iter().enumerate().take(2) {
                if count > 1 {
                    let score = match i {
                        0 => 0.6,
                        _ => 0.4,
                    };
                    candidates.push((word, score));
                }
            }

            candidates.truncate(3);
            candidates
        }
    }

    /// Compute cosine similarity between two vectors.
    fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
        let dot: f32 = a.iter().zip(b).map(|(x, y)| x * y).sum();
        let mag_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
        let mag_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();

        if mag_a == 0.0 || mag_b == 0.0 {
            0.0
        } else {
            dot / (mag_a * mag_b)
        }
    }
}
