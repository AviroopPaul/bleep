//! Text out of files, shared by Drive and Files on this Mac: PDFs and plain text, cut to a size
//! the model can take.

/// The most text one read returns. Longer files are cut, and the result says so.
pub const MAX_CHARS: usize = 60_000;
/// The biggest file bleep reads.
pub const MAX_BYTES: u64 = 20 * 1024 * 1024;

/// Cuts at a char boundary. Returns the text and whether it was cut.
pub fn cap(text: &str) -> (String, bool) {
    match text.char_indices().nth(MAX_CHARS) {
        Some((i, _)) => (text[..i].to_string(), true),
        None => (text.to_string(), false),
    }
}

/// Drops runs of blank lines and trailing spaces that PDF extraction leaves.
fn tidy(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut blank = 0;
    for line in text.lines().map(str::trim_end) {
        if line.trim().is_empty() {
            blank += 1;
            if blank > 1 {
                continue;
            }
        } else {
            blank = 0;
        }
        out.push_str(line);
        out.push('\n');
    }
    out.trim().to_string()
}

/// The text of a PDF. Runs off the async threads; a PDF that breaks the parser is an error,
/// not a crash. A scanned PDF has no text layer, and the error says so.
pub async fn pdf_text(bytes: Vec<u8>) -> Result<String, String> {
    let r = tokio::task::spawn_blocking(move || pdf_extract::extract_text_from_mem(&bytes)).await;
    match r {
        Ok(Ok(t)) if !t.trim().is_empty() => Ok(tidy(&t)),
        Ok(Ok(_)) => Err("This PDF has no text to read. It may be a scan.".into()),
        Ok(Err(e)) => Err(format!("Could not read this PDF: {e}")),
        Err(_) => Err("Could not read this PDF.".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn reads_the_text_of_a_pdf() {
        let bytes = std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/files/bill.pdf")).unwrap();
        let t = pdf_text(bytes).await.unwrap();
        assert!(t.contains("Electricity bill") && t.contains("1,240 INR") && t.contains("14 Dec 2026"), "{t}");
    }

    #[tokio::test]
    async fn a_broken_pdf_is_an_error() {
        assert!(pdf_text(b"%PDF-1.4 not really".to_vec()).await.is_err());
        assert!(pdf_text(Vec::new()).await.is_err());
    }

    #[test]
    fn long_text_is_cut_on_a_char_boundary() {
        let s = "é".repeat(MAX_CHARS + 5);
        let (t, cut) = cap(&s);
        assert!(cut && t.chars().count() == MAX_CHARS);
        assert_eq!(cap("short"), ("short".to_string(), false));
    }

    #[test]
    fn tidy_keeps_one_blank_line() {
        assert_eq!(tidy("a  \n\n\n\nb\n"), "a\n\nb");
    }
}
