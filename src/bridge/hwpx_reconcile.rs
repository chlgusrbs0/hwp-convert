//! Reconciliation for the frozen legacy HWPX recovery path.
//!
//! Keep existing recovery stable, but do not use reconciliation to introduce
//! semantics that the pinned rHWP public model does not expose.

use crate::ir::{
    Block, ConversionWarning, Document, Inline, Paragraph, ParagraphRole, TextStyle, WarningCode,
};

pub(super) fn reconcile(mut primary: Document, mut fallback: Document) -> Document {
    let recovered_markers = supplement_matching_list_markers(&mut primary, &fallback);
    if recovered_markers > 0 {
        primary.warnings.retain(|warning| {
            !warning
                .message
                .contains("bullet paragraph referenced missing bullet id")
                && !warning.message.contains("exposed unusable marker")
        });
    }

    let primary_coverage = SemanticCoverage::from_document(&primary);
    let fallback_coverage = SemanticCoverage::from_document(&fallback);
    let additional = fallback_coverage.additional_labels(&primary_coverage);

    if additional.is_empty() {
        return primary;
    }

    let additional = additional.join(", ");
    let text_matches = canonical_document_text(&primary) == canonical_document_text(&fallback);
    if text_matches
        && primary_coverage.is_plain_text_only()
        && fallback_coverage.dominates(&primary_coverage)
    {
        if primary.metadata.title.is_some() {
            fallback.metadata.title = primary.metadata.title.take();
        }
        if primary.metadata.author.is_some() {
            fallback.metadata.author = primary.metadata.author.take();
        }
        for warning in primary.warnings.drain(..) {
            push_warning_once(&mut fallback, warning);
        }
        push_warning_once(
            &mut fallback,
            ConversionWarning {
                code: WarningCode::Unknown,
                message: format!(
                    "HWPX section XML fallback preserved a strict semantic superset with matching text; hwp-convert selected the fallback structure to avoid partial rHWP data loss (additional: {additional})."
                ),
            },
        );
        return fallback;
    }

    push_warning_once(
        &mut primary,
        ConversionWarning {
            code: WarningCode::Unknown,
            message: format!(
                "HWPX section XML fallback exposed additional semantic structure ({additional}), but it could not be selected without risking loss of rHWP data; hwp-convert kept the rHWP result. Conversion may omit some HWPX structure."
            ),
        },
    );
    primary
}

fn supplement_matching_list_markers(primary: &mut Document, fallback: &Document) -> usize {
    if primary.sections.len() != fallback.sections.len() {
        return 0;
    }

    primary
        .sections
        .iter_mut()
        .zip(&fallback.sections)
        .map(|(primary, fallback)| supplement_list_markers(&mut primary.blocks, &fallback.blocks))
        .sum()
}

fn supplement_list_markers(primary: &mut [Block], fallback: &[Block]) -> usize {
    let mut recovered = 0;
    for block in primary {
        match block {
            Block::Paragraph(paragraph) => {
                let Some(primary_list) = paragraph.list.as_mut() else {
                    continue;
                };
                let text = canonical_inlines_text(&paragraph.inlines);
                let mut matches = Vec::new();
                collect_matching_list_info(
                    fallback,
                    &text,
                    &primary_list.kind,
                    primary_list.level,
                    &mut matches,
                );
                let [fallback_list] = matches.as_slice() else {
                    continue;
                };
                if primary_list.kind == crate::ir::ListKind::Unordered
                    && primary_list.marker.is_none()
                    && fallback_list.marker.is_some()
                {
                    primary_list.marker.clone_from(&fallback_list.marker);
                    recovered += 1;
                }
                if primary_list.marker_format.is_none() {
                    primary_list
                        .marker_format
                        .clone_from(&fallback_list.marker_format);
                }
                if primary_list.number.is_none() {
                    primary_list.number = fallback_list.number;
                }
            }
            Block::Table(table) => {
                if let Some(caption) = &mut table.caption {
                    recovered += supplement_list_markers(&mut caption.blocks, fallback);
                }
                for row in &mut table.rows {
                    for cell in &mut row.cells {
                        recovered += supplement_list_markers(&mut cell.blocks, fallback);
                    }
                }
            }
            Block::Shape(shape) => {
                if let Some(caption) = &mut shape.caption {
                    recovered += supplement_list_markers(&mut caption.blocks, fallback);
                }
                recovered += supplement_list_markers(&mut shape.content, fallback);
                recovered += supplement_list_markers(&mut shape.children, fallback);
            }
            Block::Image(image) => {
                if let Some(caption) = &mut image.caption_content {
                    recovered += supplement_list_markers(&mut caption.blocks, fallback);
                }
            }
            _ => {}
        }
    }
    recovered
}

fn collect_matching_list_info<'a>(
    blocks: &'a [Block],
    text: &str,
    kind: &crate::ir::ListKind,
    level: u8,
    matches: &mut Vec<&'a crate::ir::ListInfo>,
) {
    for block in blocks {
        match block {
            Block::Paragraph(paragraph)
                if canonical_inlines_text(&paragraph.inlines) == text
                    && paragraph
                        .list
                        .as_ref()
                        .is_some_and(|list| &list.kind == kind && list.level == level) =>
            {
                matches.push(paragraph.list.as_ref().expect("list checked above"));
            }
            Block::Table(table) => {
                if let Some(caption) = &table.caption {
                    collect_matching_list_info(&caption.blocks, text, kind, level, matches);
                }
                for row in &table.rows {
                    for cell in &row.cells {
                        collect_matching_list_info(&cell.blocks, text, kind, level, matches);
                    }
                }
            }
            Block::Shape(shape) => {
                if let Some(caption) = &shape.caption {
                    collect_matching_list_info(&caption.blocks, text, kind, level, matches);
                }
                collect_matching_list_info(&shape.content, text, kind, level, matches);
                collect_matching_list_info(&shape.children, text, kind, level, matches);
            }
            Block::Image(image) => {
                if let Some(caption) = &image.caption_content {
                    collect_matching_list_info(&caption.blocks, text, kind, level, matches);
                }
            }
            _ => {}
        }
    }
}

fn canonical_inlines_text(inlines: &[Inline]) -> String {
    let mut chunks = Vec::new();
    collect_inlines_text(inlines, &mut chunks);
    chunks
        .join(" ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn push_warning_once(document: &mut Document, warning: ConversionWarning) {
    if document
        .warnings
        .iter()
        .any(|existing| existing.code == warning.code && existing.message == warning.message)
    {
        return;
    }
    document.warnings.push(warning);
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct SemanticCoverage {
    sections: usize,
    paragraphs: usize,
    styled_paragraphs: usize,
    styled_text_runs: usize,
    line_breaks: usize,
    tabs: usize,
    list_paragraphs: usize,
    tables: usize,
    table_rows: usize,
    table_cells: usize,
    styled_tables: usize,
    styled_table_cells: usize,
    merged_cells: usize,
    images: usize,
    styled_images: usize,
    equations: usize,
    shapes: usize,
    charts: usize,
    unknown_blocks: usize,
    links: usize,
    anchors: usize,
    note_refs: usize,
    special_inlines: usize,
    unknown_inlines: usize,
    headers: usize,
    footers: usize,
    master_pages: usize,
    notes: usize,
    resources: usize,
    named_styles: usize,
}

impl SemanticCoverage {
    fn from_document(document: &Document) -> Self {
        let mut coverage = Self {
            sections: document.sections.len(),
            notes: document.notes.notes.len(),
            resources: document.resources.entries.len(),
            named_styles: document.styles.text_styles.len()
                + document.styles.paragraph_styles.len()
                + document.styles.table_styles.len()
                + document.styles.table_cell_styles.len(),
            ..Self::default()
        };

        for section in &document.sections {
            coverage.headers += section.headers.len();
            coverage.footers += section.footers.len();
            coverage.master_pages += section.master_pages.len();
            for master_page in &section.master_pages {
                coverage.count_blocks(&master_page.blocks);
            }
            for header in &section.headers {
                coverage.count_blocks(&header.blocks);
            }
            coverage.count_blocks(&section.blocks);
            for footer in &section.footers {
                coverage.count_blocks(&footer.blocks);
            }
        }
        for note in &document.notes.notes {
            coverage.count_blocks(&note.blocks);
        }

        coverage
    }

    fn count_blocks(&mut self, blocks: &[Block]) {
        for block in blocks {
            self.count_block(block);
        }
    }

    fn count_block(&mut self, block: &Block) {
        match block {
            Block::Paragraph(paragraph) => self.count_paragraph(paragraph),
            Block::ColumnLayout(_) => {}
            Block::DocumentControl(_) => {}
            Block::Table(table) => {
                self.tables += 1;
                self.styled_tables += usize::from(table.style != Default::default());
                self.table_rows += table.rows.len();
                if let Some(caption) = &table.caption {
                    self.count_blocks(&caption.blocks);
                }
                for row in &table.rows {
                    self.table_cells += row.cells.len();
                    for cell in &row.cells {
                        self.styled_table_cells +=
                            usize::from(cell.style != Default::default() || cell.is_header);
                        self.merged_cells += usize::from(cell.row_span > 1 || cell.col_span > 1);
                        self.count_blocks(&cell.blocks);
                    }
                }
            }
            Block::Image(image) => {
                self.images += 1;
                if let Some(caption) = &image.caption_content {
                    self.count_blocks(&caption.blocks);
                }
                self.styled_images += usize::from(
                    image.width.is_some()
                        || image.height.is_some()
                        || image.border.is_some()
                        || image.grayscale,
                );
            }
            Block::Equation(_) => self.equations += 1,
            Block::Shape(shape) => {
                self.shapes += 1;
                if let Some(caption) = &shape.caption {
                    self.count_blocks(&caption.blocks);
                }
                self.count_blocks(&shape.content);
                self.count_blocks(&shape.children);
            }
            Block::Chart(_) => self.charts += 1,
            Block::Unknown(_) => self.unknown_blocks += 1,
        }
    }

    fn count_paragraph(&mut self, paragraph: &Paragraph) {
        self.paragraphs += 1;
        self.list_paragraphs += usize::from(paragraph.list.is_some());
        self.styled_paragraphs += usize::from(
            paragraph.role != ParagraphRole::Body
                || paragraph.style != Default::default()
                || paragraph.style_ref.is_some()
                || paragraph.list.is_some(),
        );
        self.count_inlines(&paragraph.inlines);
    }

    fn count_inlines(&mut self, inlines: &[Inline]) {
        for inline in inlines {
            match inline {
                Inline::Text(run) => {
                    self.styled_text_runs +=
                        usize::from(run.style != TextStyle::default() || run.style_ref.is_some());
                }
                Inline::Link(link) => {
                    self.links += 1;
                    self.count_inlines(&link.inlines);
                }
                Inline::Field(_) => {}
                Inline::Ruby(_) | Inline::CharacterOverlap(_) => self.special_inlines += 1,
                Inline::Anchor { .. } => self.anchors += 1,
                Inline::FootnoteRef { .. } | Inline::EndnoteRef { .. } => self.note_refs += 1,
                Inline::Unknown(_) => self.unknown_inlines += 1,
                Inline::LineBreak => self.line_breaks += 1,
                Inline::Tab => self.tabs += 1,
            }
        }
    }

    fn is_plain_text_only(&self) -> bool {
        self.styled_paragraphs == 0
            && self.styled_text_runs == 0
            && self.list_paragraphs == 0
            && self.tables == 0
            && self.images == 0
            && self.equations == 0
            && self.shapes == 0
            && self.charts == 0
            && self.unknown_blocks == 0
            && self.links == 0
            && self.anchors == 0
            && self.note_refs == 0
            && self.special_inlines == 0
            && self.unknown_inlines == 0
            && self.headers == 0
            && self.footers == 0
            && self.master_pages == 0
            && self.notes == 0
            && self.resources == 0
            && self.named_styles == 0
    }

    fn dominates(&self, other: &Self) -> bool {
        let own = self.values();
        let other = other.values();
        own.iter().zip(other).all(|(left, right)| left >= &right)
            && own.iter().zip(other).any(|(left, right)| left > &right)
    }

    fn additional_labels(&self, other: &Self) -> Vec<&'static str> {
        const LABELS: [Option<&str>; 30] = [
            Some("sections"),
            Some("paragraphs"),
            Some("styled paragraphs"),
            Some("styled text runs"),
            Some("line breaks"),
            Some("tabs"),
            Some("list paragraphs"),
            Some("tables"),
            Some("table rows"),
            Some("table cells"),
            Some("styled tables"),
            Some("styled table cells"),
            Some("merged cells"),
            Some("images"),
            Some("styled images"),
            Some("equations"),
            Some("shapes"),
            Some("charts"),
            None,
            Some("links"),
            Some("anchors"),
            Some("note refs"),
            Some("ruby/character overlap inlines"),
            None,
            Some("headers"),
            Some("footers"),
            Some("master pages"),
            Some("notes"),
            Some("resources"),
            Some("named styles"),
        ];

        self.values()
            .into_iter()
            .zip(other.values())
            .zip(LABELS)
            .filter_map(|((left, right), label)| (left > right).then_some(label).flatten())
            .collect()
    }

    fn values(&self) -> [usize; 30] {
        [
            self.sections,
            self.paragraphs,
            self.styled_paragraphs,
            self.styled_text_runs,
            self.line_breaks,
            self.tabs,
            self.list_paragraphs,
            self.tables,
            self.table_rows,
            self.table_cells,
            self.styled_tables,
            self.styled_table_cells,
            self.merged_cells,
            self.images,
            self.styled_images,
            self.equations,
            self.shapes,
            self.charts,
            self.unknown_blocks,
            self.links,
            self.anchors,
            self.note_refs,
            self.special_inlines,
            self.unknown_inlines,
            self.headers,
            self.footers,
            self.master_pages,
            self.notes,
            self.resources,
            self.named_styles,
        ]
    }
}

fn canonical_document_text(document: &Document) -> String {
    let mut chunks = Vec::new();
    for section in &document.sections {
        for master_page in &section.master_pages {
            collect_blocks_text(&master_page.blocks, &mut chunks);
        }
        for header in &section.headers {
            collect_blocks_text(&header.blocks, &mut chunks);
        }
        collect_blocks_text(&section.blocks, &mut chunks);
        for footer in &section.footers {
            collect_blocks_text(&footer.blocks, &mut chunks);
        }
    }
    for note in &document.notes.notes {
        collect_blocks_text(&note.blocks, &mut chunks);
    }

    chunks
        .join(" ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn collect_blocks_text(blocks: &[Block], chunks: &mut Vec<String>) {
    for block in blocks {
        match block {
            Block::Paragraph(paragraph) => collect_inlines_text(&paragraph.inlines, chunks),
            Block::ColumnLayout(_) => {}
            Block::DocumentControl(control) => push_text(Some(control.fallback_text()), chunks),
            Block::Table(table) => {
                let caption_before = table.caption.as_ref().is_some_and(|caption| {
                    matches!(
                        caption.placement,
                        crate::ir::CaptionPlacement::Left | crate::ir::CaptionPlacement::Top
                    )
                });
                if caption_before {
                    collect_blocks_text(
                        &table
                            .caption
                            .as_ref()
                            .expect("caption checked above")
                            .blocks,
                        chunks,
                    );
                }
                for row in &table.rows {
                    for cell in &row.cells {
                        collect_blocks_text(&cell.blocks, chunks);
                    }
                }
                if let Some(caption) = table.caption.as_ref().filter(|_| !caption_before) {
                    collect_blocks_text(&caption.blocks, chunks);
                }
            }
            Block::Image(image) => {
                let caption_before = image.caption_content.as_ref().is_some_and(|caption| {
                    matches!(
                        caption.placement,
                        crate::ir::CaptionPlacement::Left | crate::ir::CaptionPlacement::Top
                    )
                });
                if caption_before {
                    collect_blocks_text(
                        &image
                            .caption_content
                            .as_ref()
                            .expect("caption checked above")
                            .blocks,
                        chunks,
                    );
                }
                push_text(image.alt.as_deref(), chunks);
                if let Some(caption) = image.caption_content.as_ref().filter(|_| !caption_before) {
                    collect_blocks_text(&caption.blocks, chunks);
                } else if image.caption_content.is_none() {
                    push_text(image.caption.as_deref(), chunks);
                }
            }
            Block::Equation(equation) => {
                push_text(
                    equation
                        .fallback_text
                        .as_deref()
                        .or(equation.content.as_deref()),
                    chunks,
                );
            }
            Block::Shape(shape) => {
                let caption_before = shape.caption.as_ref().is_some_and(|caption| {
                    matches!(
                        caption.placement,
                        crate::ir::CaptionPlacement::Left | crate::ir::CaptionPlacement::Top
                    )
                });
                if caption_before {
                    collect_blocks_text(
                        &shape
                            .caption
                            .as_ref()
                            .expect("caption checked above")
                            .blocks,
                        chunks,
                    );
                }
                if !shape.content.is_empty() {
                    collect_blocks_text(&shape.content, chunks);
                } else if shape.children.is_empty() {
                    push_text(
                        shape
                            .fallback_text
                            .as_deref()
                            .or(shape.description.as_deref()),
                        chunks,
                    );
                } else {
                    collect_blocks_text(&shape.children, chunks);
                }
                if let Some(caption) = shape.caption.as_ref().filter(|_| !caption_before) {
                    collect_blocks_text(&caption.blocks, chunks);
                }
            }
            Block::Chart(chart) => {
                push_text(
                    chart.fallback_text.as_deref().or(chart.title.as_deref()),
                    chunks,
                );
            }
            Block::Unknown(_) => {}
        }
    }
}

fn collect_inlines_text(inlines: &[Inline], chunks: &mut Vec<String>) {
    for inline in inlines {
        match inline {
            Inline::Text(run) => push_text(Some(&run.text), chunks),
            Inline::Link(link) => {
                if link.inlines.is_empty() {
                    push_text(Some(&link.url), chunks);
                } else {
                    collect_inlines_text(&link.inlines, chunks);
                }
            }
            Inline::Field(field) => push_text(Some(&field.fallback_text), chunks),
            Inline::Ruby(ruby) => push_text(Some(&ruby.text), chunks),
            Inline::CharacterOverlap(overlap) => push_text(Some(&overlap.characters), chunks),
            Inline::Unknown(_) => {}
            Inline::LineBreak | Inline::Tab => chunks.push(" ".to_string()),
            Inline::Anchor { .. } | Inline::FootnoteRef { .. } | Inline::EndnoteRef { .. } => {}
        }
    }
}

fn push_text(text: Option<&str>, chunks: &mut Vec<String>) {
    if let Some(text) = text.filter(|text| !text.is_empty()) {
        chunks.push(text.to_string());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::{ListInfo, ListKind, MasterPage, Section, Table, TableCell, TableRow};

    #[test]
    fn supplements_missing_list_marker_when_paragraphs_match() {
        let mut primary_paragraph = Paragraph::from_plain_text("item".to_string());
        primary_paragraph.list = Some(ListInfo {
            kind: ListKind::Unordered,
            level: 0,
            ..Default::default()
        });
        let mut primary = document_with_blocks(vec![Block::Paragraph(primary_paragraph)]);
        primary.warnings.push(ConversionWarning {
            code: WarningCode::Unknown,
            message: "rhwp bullet paragraph referenced missing bullet id 1; hwp-convert used default unordered list marker behavior.".to_string(),
        });

        let mut fallback_paragraph = Paragraph::from_plain_text("item".to_string());
        fallback_paragraph.list = Some(ListInfo {
            kind: ListKind::Unordered,
            level: 0,
            marker: Some("•".to_string()),
            ..Default::default()
        });
        let fallback = document_with_blocks(vec![Block::Paragraph(fallback_paragraph)]);

        let reconciled = reconcile(primary, fallback);
        let Block::Paragraph(paragraph) = &reconciled.sections[0].blocks[0] else {
            panic!("expected paragraph block");
        };
        assert_eq!(
            paragraph
                .list
                .as_ref()
                .and_then(|list| list.marker.as_deref()),
            Some("•")
        );
        assert!(reconciled.warnings.is_empty());
    }

    #[test]
    fn selects_structurally_richer_fallback_when_text_matches() {
        let primary = document_with_blocks(vec![paragraph("left right")]);
        let fallback = document_with_blocks(vec![table(&["left", "right"])]);

        let reconciled = reconcile(primary, fallback);

        assert!(matches!(reconciled.sections[0].blocks[0], Block::Table(_)));
        assert!(reconciled.warnings.iter().any(|warning| {
            warning.message.contains("selected the fallback structure")
                && warning.message.contains("tables")
        }));
    }

    #[test]
    fn keeps_primary_when_fallback_would_drop_style() {
        let mut styled = Paragraph::from_plain_text("left right".to_string());
        match &mut styled.inlines[0] {
            Inline::Text(run) => run.style.bold = true,
            other => panic!("expected text run, got {other:?}"),
        }
        let primary = document_with_blocks(vec![Block::Paragraph(styled)]);
        let fallback = document_with_blocks(vec![table(&["left", "right"])]);

        let reconciled = reconcile(primary, fallback);

        assert!(matches!(
            reconciled.sections[0].blocks[0],
            Block::Paragraph(_)
        ));
        assert!(reconciled.warnings.iter().any(|warning| {
            warning.message.contains("could not be selected") && warning.message.contains("tables")
        }));
    }

    #[test]
    fn keeps_structured_primary_even_when_fallback_dominates_counts() {
        let mut heading = Paragraph::from_plain_text("left right".to_string());
        heading.role = ParagraphRole::Heading { level: 1 };
        let primary = document_with_blocks(vec![Block::Paragraph(heading.clone())]);
        let fallback = document_with_blocks(vec![Block::Table(Table {
            rows: vec![TableRow {
                cells: vec![TableCell {
                    blocks: vec![Block::Paragraph(heading)],
                    ..Default::default()
                }],
                height: None,
            }],
            ..Default::default()
        })]);

        let reconciled = reconcile(primary, fallback);

        assert!(matches!(
            reconciled.sections[0].blocks[0],
            Block::Paragraph(_)
        ));
        assert!(
            reconciled.warnings[0]
                .message
                .contains("could not be selected")
        );
    }

    #[test]
    fn keeps_primary_master_page_when_fallback_has_more_body_structure() {
        let primary = Document {
            sections: vec![Section {
                master_pages: vec![MasterPage {
                    blocks: vec![paragraph("master")],
                    ..Default::default()
                }],
                ..Default::default()
            }],
            ..Default::default()
        };
        let fallback = document_with_blocks(vec![table(&["master"])]);

        let reconciled = reconcile(primary, fallback);

        assert_eq!(reconciled.sections[0].master_pages.len(), 1);
        assert!(reconciled.sections[0].blocks.is_empty());
        assert!(reconciled.warnings.iter().any(|warning| {
            warning.message.contains("could not be selected") && warning.message.contains("tables")
        }));
    }

    #[test]
    fn keeps_primary_when_fallback_loses_line_breaks() {
        let primary = document_with_blocks(vec![paragraph("left\nright")]);
        let fallback = document_with_blocks(vec![table(&["left", "right"])]);

        let reconciled = reconcile(primary, fallback);

        assert!(matches!(
            reconciled.sections[0].blocks[0],
            Block::Paragraph(_)
        ));
        assert!(
            reconciled.warnings[0]
                .message
                .contains("could not be selected")
        );
    }

    #[test]
    fn keeps_primary_when_fallback_text_differs() {
        let primary = document_with_blocks(vec![paragraph("left right")]);
        let fallback = document_with_blocks(vec![table(&["left", "changed"])]);

        let reconciled = reconcile(primary, fallback);

        assert!(matches!(
            reconciled.sections[0].blocks[0],
            Block::Paragraph(_)
        ));
        assert_eq!(reconciled.warnings.len(), 1);
    }

    #[test]
    fn leaves_equivalent_primary_unchanged() {
        let primary = document_with_blocks(vec![paragraph("same")]);
        let fallback = document_with_blocks(vec![paragraph("same")]);

        let reconciled = reconcile(primary.clone(), fallback);

        assert_eq!(reconciled, primary);
    }

    fn document_with_blocks(blocks: Vec<Block>) -> Document {
        Document {
            sections: vec![Section {
                blocks,
                ..Default::default()
            }],
            ..Default::default()
        }
    }

    fn paragraph(text: &str) -> Block {
        Block::Paragraph(Paragraph::from_plain_text(text.to_string()))
    }

    fn table(cell_texts: &[&str]) -> Block {
        Block::Table(Table {
            rows: vec![TableRow {
                cells: cell_texts
                    .iter()
                    .map(|text| TableCell {
                        blocks: vec![paragraph(text)],
                        ..Default::default()
                    })
                    .collect(),
                height: None,
            }],
            ..Default::default()
        })
    }
}
