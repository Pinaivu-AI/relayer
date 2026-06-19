//! Parses the `KEY=VALUE` lines the parent sends over VSOCK:7000.

use std::io::BufRead;

pub fn read_config<R: BufRead>(reader: R) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for line in reader.lines() {
        let Ok(line) = line else { break };
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let Some((key, val)) = trimmed.split_once('=') else { continue };
        let key = key.trim();
        if key.is_empty() {
            continue;
        }
        out.push((key.to_string(), val.trim().trim_matches('"').to_string()));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::BufReader;

    #[test]
    fn parses_pairs_and_skips_noise() {
        let input = b"# comment\n\nDATABASE_URL=postgres://x\nQUOTED=\"abc def\"\n";
        let pairs = read_config(BufReader::new(input.as_ref()));
        assert_eq!(
            pairs,
            vec![
                ("DATABASE_URL".into(), "postgres://x".into()),
                ("QUOTED".into(), "abc def".into()),
            ]
        );
    }

    #[test]
    fn value_with_embedded_equals_kept_whole() {
        let input = b"URL=postgres://u:p@h/db?sslmode=require\n";
        let pairs = read_config(BufReader::new(input.as_ref()));
        assert_eq!(pairs[0].1, "postgres://u:p@h/db?sslmode=require");
    }
}
