use crate::ir::{
    Block, Chart, Document, Equation, EquationKind, HeaderFooter, Image, Inline, ListInfo,
    ListKind, Note, NoteKind, Paragraph, Shape, Table, TableCell, UnknownBlock, UnknownInline,
};

const TABLE_FALLBACK_LABEL: &str = "[\u{D45C}]";
const HEADER_FALLBACK_LABEL: &str = "[\u{BA38}\u{B9AC}\u{B9D0}]";
const FOOTER_FALLBACK_LABEL: &str = "[\u{AF2C}\u{B9AC}\u{B9D0}]";
const FOOTNOTE_REF_LABEL: &str = "[\u{AC01}\u{C8FC}";
const ENDNOTE_REF_LABEL: &str = "[\u{BBF8}\u{C8FC}";

#[allow(dead_code)]
pub fn collect_paragraph_texts(document: &Document) -> Vec<String> {
    let mut paragraphs = Vec::new();

    for section in &document.sections {
        for block in &section.blocks {
            if let Block::Paragraph(paragraph) = block {
                paragraphs.push(paragraph_to_plain_text(paragraph));
            }
        }
    }

    paragraphs
}

pub fn collect_block_texts(document: &Document) -> Vec<String> {
    let mut blocks = Vec::new();

    for section in &document.sections {
        blocks.extend(
            section
                .headers
                .iter()
                .map(|header| header_footer_to_plain_text(HEADER_FALLBACK_LABEL, header)),
        );

        for block in &section.blocks {
            if !matches!(block, Block::ColumnLayout(_)) {
                blocks.push(block_to_plain_text(block));
            }
        }

        blocks.extend(
            section
                .footers
                .iter()
                .map(|footer| header_footer_to_plain_text(FOOTER_FALLBACK_LABEL, footer)),
        );
    }

    for note in &document.notes.notes {
        blocks.push(note_to_plain_text(note));
    }

    blocks
}

pub fn to_plain_text(document: &Document) -> String {
    collect_block_texts(document).join("\n")
}

pub(crate) fn block_to_plain_text(block: &Block) -> String {
    match block {
        Block::Paragraph(paragraph) => paragraph_to_plain_text(paragraph),
        Block::ColumnLayout(_) => String::new(),
        Block::DocumentControl(control) => control.fallback_text().to_string(),
        Block::Table(table) => table_to_plain_text(table),
        Block::Image(image) => image_to_plain_text(image),
        Block::Equation(equation) => equation_to_plain_text(equation),
        Block::Shape(shape) => shape_to_plain_text(shape),
        Block::Chart(chart) => chart_to_plain_text(chart),
        Block::Unknown(unknown) => unknown_block_to_plain_text(unknown),
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

    let mut text = format!("[\u{C774}\u{BBF8}\u{C9C0}: {label}]");
    if let Some(caption) = image
        .caption
        .as_deref()
        .filter(|caption| !caption.is_empty())
    {
        text.push('\n');
        text.push_str(caption);
    }
    text
}

pub(crate) fn equation_to_plain_text(equation: &Equation) -> String {
    let text = equation
        .fallback_text
        .as_deref()
        .filter(|text| !text.is_empty())
        .map(ToOwned::to_owned)
        .or_else(|| {
            if matches!(
                equation.kind,
                EquationKind::PlainText | EquationKind::Latex | EquationKind::MathMl
            ) {
                equation
                    .content
                    .as_deref()
                    .filter(|text| !text.is_empty())
                    .map(ToOwned::to_owned)
            } else {
                None
            }
        })
        .unwrap_or_else(|| "unsupported".to_string());

    format!("[equation: {text}]")
}

pub(crate) fn shape_to_plain_text(shape: &Shape) -> String {
    let text = shape
        .fallback_text
        .as_deref()
        .filter(|text| !text.is_empty())
        .map(ToOwned::to_owned)
        .or_else(|| {
            shape
                .description
                .as_deref()
                .filter(|text| !text.is_empty())
                .map(ToOwned::to_owned)
        })
        .unwrap_or_else(|| "unsupported".to_string());

    format!("[shape: {text}]")
}

pub(crate) fn chart_to_plain_text(chart: &Chart) -> String {
    let text = chart
        .fallback_text
        .as_deref()
        .filter(|text| !text.is_empty())
        .map(ToOwned::to_owned)
        .or_else(|| {
            chart
                .title
                .as_deref()
                .filter(|text| !text.is_empty())
                .map(ToOwned::to_owned)
        })
        .unwrap_or_else(|| "unsupported".to_string());

    format!("[chart: {text}]")
}

pub(crate) fn unknown_block_to_plain_text(unknown: &UnknownBlock) -> String {
    unknown
        .fallback_text
        .as_deref()
        .filter(|text| !text.is_empty())
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| format!("[unknown: {}]", unknown.kind))
}

fn unknown_inline_to_plain_text(unknown: &UnknownInline) -> String {
    unknown
        .fallback_text
        .as_deref()
        .filter(|text| !text.is_empty())
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| format!("[unknown: {}]", unknown.kind))
}

pub(crate) fn header_footer_to_plain_text(label: &str, header_footer: &HeaderFooter) -> String {
    let content = blocks_to_plain_text(&header_footer.blocks);

    if content.is_empty() {
        label.to_string()
    } else {
        format!("{label}\n{content}")
    }
}

pub(crate) fn note_to_plain_text(note: &Note) -> String {
    let label = match note.kind {
        NoteKind::Footnote => FOOTNOTE_REF_LABEL,
        NoteKind::Endnote => ENDNOTE_REF_LABEL,
    };
    let content = blocks_to_plain_text(&note.blocks);

    if content.is_empty() {
        format!("{label}: {}]", note.id.as_str())
    } else {
        format!("{label}: {}]\n{content}", note.id.as_str())
    }
}

fn table_cell_to_plain_text(cell: &TableCell) -> String {
    blocks_to_plain_text(&cell.blocks)
}

fn paragraph_to_plain_text(paragraph: &Paragraph) -> String {
    let mut text = inline_text_to_plain_text(&paragraph.inlines);

    if let Some(list) = &paragraph.list {
        text.insert_str(0, &list_prefix_to_plain_text(list));
    }

    text
}

fn list_prefix_to_plain_text(list: &ListInfo) -> String {
    let indent = "  ".repeat(list.level as usize);

    let marker = match list.kind {
        ListKind::Ordered => list
            .marker
            .as_deref()
            .map(plain_text_marker_with_space)
            .unwrap_or_else(|| format!("{}. ", list.number.unwrap_or(1))),
        ListKind::Unordered | ListKind::Unknown => {
            plain_text_marker_with_space(list.marker.as_deref().unwrap_or("-"))
        }
    };

    format!("{indent}{marker}")
}

fn plain_text_marker_with_space(marker: &str) -> String {
    if marker.chars().last().is_some_and(char::is_whitespace) {
        marker.to_string()
    } else {
        format!("{marker} ")
    }
}

fn inline_text_to_plain_text(inlines: &[Inline]) -> String {
    let mut text = String::new();

    for inline in inlines {
        match inline {
            Inline::Text(run) => text.push_str(&run.text),
            Inline::LineBreak => text.push('\n'),
            Inline::Tab => text.push('\t'),
            Inline::Link(link) => {
                let link_text = inline_text_to_plain_text(&link.inlines);
                if link_text.is_empty() {
                    text.push_str(&link.url);
                } else {
                    text.push_str(&link_text);
                }
            }
            Inline::FootnoteRef { note_id } => {
                text.push_str(&format!("{FOOTNOTE_REF_LABEL}: {}]", note_id.as_str()));
            }
            Inline::EndnoteRef { note_id } => {
                text.push_str(&format!("{ENDNOTE_REF_LABEL}: {}]", note_id.as_str()));
            }
            Inline::Anchor { .. } => {}
            Inline::Unknown(unknown) => {
                text.push_str(&unknown_inline_to_plain_text(unknown));
            }
        }
    }

    text
}

pub fn sanitize_anchor_id(name: &str) -> String {
    let mut id = String::new();
    for ch in name.chars() {
        if ch.is_alphanumeric() || ch == '-' || ch == '_' || ch == ':' || ch == '.' {
            id.push(ch);
        } else if ch.is_whitespace() {
            id.push('-');
        }
    }
    if id.is_empty() {
        "bookmark".to_string()
    } else {
        id
    }
}

#[cfg(test)]
mod tests {
    use crate::ir::{
        Block, Chart, Document, Equation, EquationKind, HeaderFooter, Image, Inline, ListInfo,
        ListKind, Note, NoteId, NoteKind, Paragraph, ParagraphRole, ParagraphStyle, ResourceId,
        Shape, ShapeKind, Table, TableCell, TableCellStyle, TableRow, TableStyle, TextRun,
        TextStyle,
    };

    use super::to_plain_text;

    #[test]
    fn rebuilds_plain_text_from_document() {
        let document = Document::from_paragraphs(vec!["a".to_string(), "b".to_string()]);

        assert_eq!(to_plain_text(&document), "a\nb");
    }

    #[test]
    fn preserves_empty_paragraphs_as_blank_lines() {
        let document = Document::from_paragraphs(vec![
            "before".to_string(),
            String::new(),
            "after".to_string(),
        ]);

        assert_eq!(to_plain_text(&document), "before\n\nafter");
    }

    #[test]
    fn renders_table_as_plain_text_fallback() {
        let document = Document {
            sections: vec![crate::ir::Section {
                blocks: vec![Block::Table(Table {
                    rows: vec![
                        TableRow {
                            cells: vec![table_cell("cell1"), table_cell("cell2")],
                            height: None,
                        },
                        TableRow {
                            cells: vec![table_cell("cell3"), table_cell("cell4")],
                            height: None,
                        },
                    ],
                    style: TableStyle::default(),
                })],
                ..Default::default()
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
                    caption: Some("company mark".to_string()),
                    ..Default::default()
                })],
                ..Default::default()
            }],
            ..Default::default()
        };

        assert_eq!(
            to_plain_text(&document),
            "[\u{C774}\u{BBF8}\u{C9C0}: logo]\ncompany mark"
        );
    }

    #[test]
    fn renders_links_note_refs_and_lists_in_plain_text() {
        let document = Document {
            sections: vec![crate::ir::Section {
                blocks: vec![Block::Paragraph(Paragraph {
                    role: ParagraphRole::Body,
                    inlines: vec![
                        Inline::Link(crate::ir::Link {
                            url: "https://example.com".to_string(),
                            title: None,
                            inlines: vec![Inline::Text(TextRun {
                                text: "link".to_string(),
                                style: TextStyle::default(),
                                style_ref: None,
                            })],
                        }),
                        Inline::Text(TextRun {
                            text: " ".to_string(),
                            style: TextStyle::default(),
                            style_ref: None,
                        }),
                        Inline::FootnoteRef {
                            note_id: NoteId("fn-1".to_string()),
                        },
                        Inline::Text(TextRun {
                            text: " ".to_string(),
                            style: TextStyle::default(),
                            style_ref: None,
                        }),
                        Inline::EndnoteRef {
                            note_id: NoteId("en-1".to_string()),
                        },
                    ],
                    style: ParagraphStyle::default(),
                    style_ref: None,
                    list: Some(ListInfo {
                        kind: ListKind::Ordered,
                        level: 0,
                        marker: None,
                        marker_format: None,
                        number: Some(3),
                    }),
                })],
                ..Default::default()
            }],
            ..Default::default()
        };

        assert_eq!(
            to_plain_text(&document),
            "3. link [\u{AC01}\u{C8FC}: fn-1] [\u{BBF8}\u{C8FC}: en-1]"
        );
    }

    #[test]
    fn omits_anchor_inlines_from_plain_text() {
        let document = Document {
            sections: vec![crate::ir::Section {
                blocks: vec![Block::Paragraph(Paragraph {
                    role: ParagraphRole::Body,
                    inlines: vec![
                        Inline::Text(TextRun {
                            text: "before".to_string(),
                            style: TextStyle::default(),
                            style_ref: None,
                        }),
                        Inline::Anchor {
                            id: "bookmark-1".to_string(),
                        },
                        Inline::Text(TextRun {
                            text: " after".to_string(),
                            style: TextStyle::default(),
                            style_ref: None,
                        }),
                    ],
                    style: ParagraphStyle::default(),
                    style_ref: None,
                    list: None,
                })],
                ..Default::default()
            }],
            ..Default::default()
        };

        assert_eq!(to_plain_text(&document), "before after");
    }

    #[test]
    fn sanitizes_anchor_ids_without_dropping_korean_text() {
        assert_eq!(super::sanitize_anchor_id("본문 앵커 1"), "본문-앵커-1");
    }

    #[test]
    fn renders_headers_footers_and_notes_in_plain_text() {
        let document = Document {
            sections: vec![crate::ir::Section {
                headers: vec![HeaderFooter {
                    placement: crate::ir::HeaderFooterPlacement::Default,
                    blocks: vec![Block::Paragraph(Paragraph {
                        role: ParagraphRole::Body,
                        inlines: vec![Inline::Text(TextRun {
                            text: "header".to_string(),
                            style: TextStyle::default(),
                            style_ref: None,
                        })],
                        style: ParagraphStyle::default(),
                        style_ref: None,
                        list: None,
                    })],
                }],
                blocks: vec![Block::Paragraph(Paragraph {
                    role: ParagraphRole::Body,
                    inlines: vec![Inline::Text(TextRun {
                        text: "body".to_string(),
                        style: TextStyle::default(),
                        style_ref: None,
                    })],
                    style: ParagraphStyle::default(),
                    style_ref: None,
                    list: None,
                })],
                footers: vec![HeaderFooter {
                    placement: crate::ir::HeaderFooterPlacement::Default,
                    blocks: vec![Block::Paragraph(Paragraph {
                        role: ParagraphRole::Body,
                        inlines: vec![Inline::Text(TextRun {
                            text: "footer".to_string(),
                            style: TextStyle::default(),
                            style_ref: None,
                        })],
                        style: ParagraphStyle::default(),
                        style_ref: None,
                        list: None,
                    })],
                }],
                layout: None,
            }],
            notes: crate::ir::NoteStore {
                notes: vec![Note {
                    id: NoteId("fn-1".to_string()),
                    kind: NoteKind::Footnote,
                    blocks: vec![Block::Paragraph(Paragraph {
                        role: ParagraphRole::Body,
                        inlines: vec![Inline::Text(TextRun {
                            text: "note body".to_string(),
                            style: TextStyle::default(),
                            style_ref: None,
                        })],
                        style: ParagraphStyle::default(),
                        style_ref: None,
                        list: None,
                    })],
                }],
            },
            ..Default::default()
        };

        assert_eq!(
            to_plain_text(&document),
            "[\u{BA38}\u{B9AC}\u{B9D0}]\nheader\nbody\n[\u{AF2C}\u{B9AC}\u{B9D0}]\nfooter\n[\u{AC01}\u{C8FC}: fn-1]\nnote body"
        );
    }

    #[test]
    fn renders_equation_shape_and_chart_fallbacks_in_plain_text() {
        let document = Document {
            sections: vec![crate::ir::Section {
                blocks: vec![
                    Block::Equation(Equation {
                        kind: EquationKind::Unknown,
                        content: None,
                        fallback_text: Some("x + y".to_string()),
                        resource_id: None,
                        ..Default::default()
                    }),
                    Block::Shape(Shape {
                        kind: ShapeKind::Rectangle,
                        fallback_text: None,
                        description: Some("callout box".to_string()),
                        ..Default::default()
                    }),
                    Block::Chart(Chart {
                        title: Some("Sales".to_string()),
                        fallback_text: None,
                        resource_id: None,
                    }),
                ],
                ..Default::default()
            }],
            ..Default::default()
        };

        assert_eq!(
            to_plain_text(&document),
            "[equation: x + y]\n[shape: callout box]\n[chart: Sales]"
        );
    }

    fn table_cell(text: &str) -> TableCell {
        TableCell {
            row_span: 1,
            col_span: 1,
            is_header: false,
            blocks: vec![Block::Paragraph(Paragraph {
                role: ParagraphRole::Body,
                inlines: vec![crate::ir::Inline::Text(TextRun {
                    text: text.to_string(),
                    style: TextStyle::default(),
                    style_ref: None,
                })],
                style: ParagraphStyle::default(),
                style_ref: None,
                list: None,
            })],
            style: TableCellStyle::default(),
        }
    }
}
