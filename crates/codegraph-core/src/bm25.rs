use std::collections::HashMap;

pub struct BM25Index {
    k1: f32,
    b: f32,
    doc_lengths: Vec<usize>,
    avg_doc_length: f32,
    postings: HashMap<String, Vec<(usize, u32)>>, // term -> [(doc_id, term_freq)]
    doc_count: usize,
}

impl BM25Index {
    pub fn new() -> Self {
        Self {
            k1: 1.2,
            b: 0.75,
            doc_lengths: Vec::new(),
            avg_doc_length: 0.0,
            postings: HashMap::new(),
            doc_count: 0,
        }
    }

    /// Indexes a document text (signature + docstring) with an assigned doc_id
    pub fn add_document(&mut self, doc_id: usize, text: &str) {
        let tokens = tokenize(text);
        let doc_len = tokens.len();

        if doc_id >= self.doc_lengths.len() {
            self.doc_lengths.resize(doc_id + 1, 0);
        }
        self.doc_lengths[doc_id] = doc_len;
        self.doc_count = self.doc_count.max(doc_id + 1);

        let mut term_freqs: HashMap<String, u32> = HashMap::new();
        for t in tokens {
            *term_freqs.entry(t).or_insert(0) += 1;
        }

        for (term, freq) in term_freqs {
            self.postings.entry(term).or_default().push((doc_id, freq));
        }

        let total_len: usize = self.doc_lengths.iter().sum();
        self.avg_doc_length = if self.doc_count > 0 {
            total_len as f32 / self.doc_count as f32
        } else {
            0.0
        };
    }

    /// Scores all documents against a query string using Okapi BM25
    pub fn query(&self, query_str: &str, top_k: usize) -> Vec<(usize, f32)> {
        if self.doc_count == 0 {
            return Vec::new();
        }

        let query_tokens = tokenize(query_str);
        if query_tokens.is_empty() {
            return Vec::new();
        }

        let mut scores: HashMap<usize, f32> = HashMap::new();
        let total_docs = self.doc_count as f32;

        for token in &query_tokens {
            if let Some(posting_list) = self.postings.get(token) {
                let n_q = posting_list.len() as f32;
                // Standard Lucene / Okapi IDF
                let idf = ((total_docs - n_q + 0.5) / (n_q + 0.5) + 1.0).ln();

                for &(doc_id, freq) in posting_list {
                    let doc_len = self.doc_lengths[doc_id] as f32;
                    let denom = freq as f32
                        + self.k1 * (1.0 - self.b + self.b * (doc_len / self.avg_doc_length.max(1.0)));
                    let term_score = idf * (freq as f32 * (self.k1 + 1.0)) / denom;

                    *scores.entry(doc_id).or_insert(0.0) += term_score;
                }
            }
        }

        let mut ranked: Vec<(usize, f32)> = scores.into_iter().collect();
        ranked.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        ranked.truncate(top_k);
        ranked
    }
}

/// Tokenizer that splits snake_case, camelCase, and punctuation into lowercase search tokens
pub fn tokenize(text: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current_token = String::new();

    for (i, c) in text.chars().enumerate() {
        if c.is_alphanumeric() {
            // Check camelCase boundary (e.g. formatCurrency -> format, Currency)
            if c.is_uppercase() && !current_token.is_empty() {
                // If previous char is lowercase, split
                if let Some(prev) = text.chars().nth(i.saturating_sub(1)) {
                    if prev.is_lowercase() {
                        tokens.push(current_token.to_lowercase());
                        current_token = String::new();
                    }
                }
            }
            current_token.push(c);
        } else {
            if !current_token.is_empty() {
                tokens.push(current_token.to_lowercase());
                current_token = String::new();
            }
        }
    }

    if !current_token.is_empty() {
        tokens.push(current_token.to_lowercase());
    }

    // Filter out common code stop words
    tokens
        .into_iter()
        .filter(|t| t.len() > 1 && !is_stop_word(t))
        .collect()
}

fn is_stop_word(term: &str) -> bool {
    matches!(
        term,
        "function"
            | "const"
            | "let"
            | "var"
            | "pub"
            | "fn"
            | "def"
            | "class"
            | "type"
            | "interface"
            | "return"
            | "async"
            | "await"
            | "export"
            | "import"
            | "this"
            | "self"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tokenizer_camel_snake() {
        let tokens = tokenize("export async function formatCurrency_usd(val: number)");
        assert!(tokens.contains(&"currency".to_string()));
        assert!(tokens.contains(&"format".to_string()));
        assert!(tokens.contains(&"usd".to_string()));
    }

    #[test]
    fn test_bm25_query() {
        let mut index = BM25Index::new();
        index.add_document(0, "format currency with locale and symbol");
        index.add_document(1, "validate JWT authorization session token");
        index.add_document(2, "retry HTTP request with exponential backoff");

        let results = index.query("currency format", 1);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].0, 0);

        let results_retry = index.query("exponential retry", 1);
        assert_eq!(results_retry.len(), 1);
        assert_eq!(results_retry[0].0, 2);
    }
}
