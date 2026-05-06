/// Extract `<<IMG:path>>` markers from text.
///
/// Returns `(cleaned_text, vec_of_paths)` — the text has all markers removed
/// and trimmed, the vec contains the file paths in order of appearance.
pub fn extract_img_markers(text: &str) -> (String, Vec<String>) {
    extract_markers_with_prefix(text, "<<IMG:")
}

/// Extract `<<VID:path>>` markers from text — mirror of `extract_img_markers`
/// for video attachments. Used by channel handlers to strip the marker from
/// bot replies before display (the agent shouldn't normally echo it back, but
/// strip defensively so a leaking marker never lands in front of the user).
pub fn extract_vid_markers(text: &str) -> (String, Vec<String>) {
    extract_markers_with_prefix(text, "<<VID:")
}

/// Generic `<<PREFIX:path>>` marker extractor. Walks the text, removes every
/// `<<PREFIX:...>>` occurrence, and collects the inner paths in order. UTF-8
/// safe (works on byte indices that lie on char boundaries — `find`/`replace_range`
/// handle that correctly for the ASCII delimiters used here).
fn extract_markers_with_prefix(text: &str, prefix: &str) -> (String, Vec<String>) {
    let mut out = text.to_string();
    let mut paths = Vec::new();
    let prefix_len = prefix.len();

    while let Some(start) = out.find(prefix) {
        let Some(rel_end) = out[start..].find(">>") else {
            break;
        };
        let end = start + rel_end + 2; // past ">>"
        let path = out[start + prefix_len..start + rel_end].trim().to_string();
        if !path.is_empty() {
            paths.push(path);
        }
        out.replace_range(start..end, "");
    }

    (out.trim().to_string(), paths)
}
