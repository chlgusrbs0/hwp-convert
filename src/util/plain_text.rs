use crate::ir::{Block, Document, Image, Inline, Table, TableCell};

const TABLE_FALLBACK_LABEL: &str = "[\u{D45C}]";

#[allow(dead_code)]
pub fn collect_paragraph_texts(document: &Document) -> Vec<String> {
    let mut paragraphs = Vec::new();

    for section in &document.sections {
        for block in &section.blocks {
            if let Block::Paragraph(paragraph) = block {
                paragraphs.push(inline_text_to_plain_text(&paragraph.inlines));
            }
        }
    }

    paragraphs
}

pub fn collect_block_texts(document: &Document) -> Vec<String> {
    let mut blocks = Vec::new();

    for section in &document.sections {
        for block in &section.blocks {
            blocks.push(block_to_plain_text(block));
        }
    }

    blocks
}

pub fn to_plain_text(document: &Document) -> String {
    collect_block_texts(document).join("\n")
}

pub(crate) fn block_to_plain_text(block: &Block) -> String {
    match block {
        Block::Paragraph(paragraph) => inline_text_to_plain_text(&paragraph.inlines),
        Block::Table(table) => table_to_plain_text(table),
        Block::Image(image) => image_to_plain_text(image),
        Block::Unknown(unknown) => unknown.fallback_text.clone().unwrap_or_default(),
    }
}

pub(crate) fn blocks_to_plain_text(blocks: &[Block]) -> String {
    blocks
        .iter()
        .map(block_to_plain_text)
        .filter(|text| !text.is_empty())
        .map(|text| text.replace('\n', " "))
        .collect::<Vec<_>>()
        .join(" ")
}

pub(crate) fn table_to_plain_text(table: &Table) -> String {
    let mut lines = vec![TABLE_FALLBACK_LABEL.to_string()];

    for row in &table.rows {
        lines.push(
            row.cells
                .iter()
                .map(table_cell_to_plain_text)
                .collect::<Vec<_>>()
                .join(" | "),
        );
    }

    lines.join("\n")
}

pub(crate) fn image_to_plain_text(image: &Image) -> String {
    let label = image
        .alt
        .as_deref()
        .filter(|alt| !alt.is_empty())
        .unwrap_or_else(|| image.resource_id.as_str());

    format!("[\u{C774}\u{BBF8}\u{C9C0}: {label}]")
}

fn table_cell_to_plain_text(cell: &TableCell) -> String {
    blocks_to_plain_text(&cell.blocks)
}

fn inline_text_to_plain_text(inlines: &[Inline]) -> String {
    let mut text = String::new();

    for inline in inlines {
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

    text
}

#[cfg(test)]
mod tests {
    use crate::ir::{
        Block, Document, Image, Paragraph, ParagraphRole, ParagraphStyle, ResourceId, Table,
        TableCell, TableCellStyle, TableRow, TableStyle, TextRun, TextStyle,
    };

    use super::to_plain_text;

    #[test]
    fn rebuilds_plain_text_from_document() {
        let document = Document::from_paragraphs(vec!["a".to_string(), "b".to_string()]);

        assert_eq!(to_plain_text(&document), "a\nb");
    }

    #[test]
    fn renders_table_as_plain_text_fallback() {
        let document = Document {
            sections: vec![crate::ir::Section {
                blocks: vec![Block::Table(Table {
                    rows: vec![
                        TableRow {
                            cells: vec![table_cell("cell1"), table_cell("cell2")],
                        },
                        TableRow {
                            cells: vec![table_cell("cell3"), table_cell("cell4")],
                        },
                    ],
                    style: TableStyle::default(),
                })],
            }],
            ..Default::default()
        };

        assert_eq!(
            to_plain_text(&document),
            "[\u{D45C}]\ncell1 | cell2\ncell3 | cell4"
        );
    }

    #[test]
    fn renders_image_as_plain_text_fallback() {
        let document = Document {
            sections: vec![crate::ir::Section {
                blocks: vec![Block::Image(Image {
                    resource_id: ResourceId("image-1".to_string()),
                    alt: Some("logo".to_string()),
                    caption: None,
                    width: None,
                    height: None,
                })],
            }],
            ..Default::default()
        };

        assert_eq!(to_plain_text(&document), "[\u{C774}\u{BBF8}\u{C9C0}: logo]");
    }

    fn table_cell(text: &str) -> TableCell {
        TableCell {
            row_span: 1,
            col_span: 1,
            blocks: vec![Block::Paragraph(Paragraph {
                role: ParagraphRole::Body,
                inlines: vec![crate::ir::Inline::Text(TextRun {
                    text: text.to_string(),
                    style: TextStyle::default(),
                    style_ref: None,
                })],
                style: ParagraphStyle::default(),
                style_ref: None,
            })],
            style: TableCellStyle::default(),
        }
    }
}
