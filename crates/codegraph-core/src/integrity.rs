use regex::Regex;
use serde::{Deserialize, Serialize};
use std::sync::OnceLock;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelaxedAssertion {
    pub file: String,
    pub line: usize,
    pub original: String,
    pub relaxed_to: String,
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssertionAuditReport {
    pub is_clean: bool,
    pub relaxed_count: usize,
    pub violations: Vec<RelaxedAssertion>,
    pub advisory_message: String,
}

static STRICT_ASSERTION_PATTERNS: OnceLock<Vec<Regex>> = OnceLock::new();
static RELAXED_ASSERTION_PATTERNS: OnceLock<Vec<Regex>> = OnceLock::new();

fn get_strict_patterns() -> &'static [Regex] {
    STRICT_ASSERTION_PATTERNS.get_or_init(|| {
        vec![
            Regex::new(r"(?i)\.toBe\(").unwrap(),
            Regex::new(r"(?i)\.toStrictEqual\(").unwrap(),
            Regex::new(r"(?i)\.toEqual\(").unwrap(),
            Regex::new(r"(?i)assert_eq!\(").unwrap(),
            Regex::new(r"(?i)self\.assertEqual\(").unwrap(),
            Regex::new(r"assert\s+[a-zA-Z0-9_]+\s*==\s*").unwrap(),
        ]
    })
}

fn get_relaxed_patterns() -> &'static [Regex] {
    RELAXED_ASSERTION_PATTERNS.get_or_init(|| {
        vec![
            Regex::new(r"(?i)\.toBeDefined\(\)").unwrap(),
            Regex::new(r"(?i)\.toBeTruthy\(\)").unwrap(),
            Regex::new(r"(?i)\.not\.toBeNull\(\)").unwrap(),
            Regex::new(r"(?i)assert\s+[a-zA-Z0-9_]+\s+is\s+not\s+None").unwrap(),
        ]
    })
}

/// Audits modified test file content against original baseline to detect relaxed assertions
pub fn audit_test_diff(file_path: &str, old_content: &str, new_content: &str) -> AssertionAuditReport {
    let mut violations = Vec::new();

    let old_has_strict = get_strict_patterns().iter().any(|r| r.is_match(old_content));
    let _new_has_relaxed = get_relaxed_patterns().iter().any(|r| r.is_match(new_content));

    // Simple line-by-line inspection
    let old_lines: Vec<&str> = old_content.lines().collect();
    let new_lines: Vec<&str> = new_content.lines().collect();

    for (line_idx, new_line) in new_lines.iter().enumerate() {
        if get_relaxed_patterns().iter().any(|r| r.is_match(new_line)) {
            // Check if corresponding old line or surrounding context had strict assertion
            if old_has_strict && !get_strict_patterns().iter().any(|r| r.is_match(new_line)) {
                let prev_strict = old_lines.iter().find(|l| get_strict_patterns().iter().any(|r| r.is_match(l)));
                violations.push(RelaxedAssertion {
                    file: file_path.to_string(),
                    line: line_idx + 1,
                    original: prev_strict.unwrap_or(&"<strict assertion>").trim().to_string(),
                    relaxed_to: new_line.trim().to_string(),
                    description: "Strict equality assertion was relaxed to a generic truthiness/defined check.".to_string(),
                });
            }
        }
    }

    let is_clean = violations.is_empty();
    let relaxed_count = violations.len();
    let advisory_message = if is_clean {
        "Test assertions are clean and strict.".to_string()
    } else {
        format!(
            "[ADVISORY WARNING] Detected {} relaxed assertion(s) in {}. Avoid weakening assertions to pass prompts.",
            relaxed_count, file_path
        )
    };

    AssertionAuditReport {
        is_clean,
        relaxed_count,
        violations,
        advisory_message,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_audit_assertion_weakening() {
        let old_test = r#"
            it("returns status 200", async () => {
                const res = await request(app).get("/api");
                expect(res.status).toBe(200);
            });
        "#;

        let new_test = r#"
            it("returns status 200", async () => {
                const res = await request(app).get("/api");
                expect(res.status).toBeDefined();
            });
        "#;

        let report = audit_test_diff("tests/api.test.ts", old_test, new_test);
        assert!(!report.is_clean);
        assert_eq!(report.relaxed_count, 1);
        assert!(report.advisory_message.contains("ADVISORY WARNING"));
    }
}
