use crate::ir::{Block, Document, Inline};

pub fn collect_paragraph_texts(document: &Document) -> Vec<String> {
    let mut paragraphs = Vec::new();

    for section in &document.sections {
        for block in &section.blocks {
            if let Block::Paragraph(paragraph) = block {
                let mut text = String::new();

                for inline in &paragraph.inlines {
                    match inline {
                        Inline::Text(run) => text.push_str(&run.text),
                        Inline::LineBreak => text.push('\n'),
                        Inline::Tab => text.push('\t'),
                        Inline::Unknown(unknown) => {
                            if let Some(fallback) = &unknown.fallback_text {
                                text.push_str(fallback);
                            }
                        }
                    }
                }

                paragraphs.push(text);
            }
        }
    }

    paragraphs
}

pub fn to_plain_text(document: &Document) -> String {
    collect_paragraph_texts(document).join("\n")
}

#[cfg(test)]
mod tests {
    use crate::ir::Document;

    use super::to_plain_text;

    #[test]
    fn rebuilds_plain_text_from_document() {
        let document = Document::from_paragraphs(vec!["a".to_string(), "b".to_string()]);

        assert_eq!(to_plain_text(&document), "a\nb");
    }
}
