use std::collections::BTreeMap;
use std::error::Error;
use std::io;
use std::path::Path;

use rhwp::model::control::{
    AutoNumberType as RhwpAutoNumberType, Bookmark as RhwpBookmark, Control,
    Equation as RhwpEquation, Field as RhwpField, FieldType as RhwpFieldType,
    FormObject as RhwpFormObject, FormType as RhwpFormType, HiddenComment as RhwpHiddenComment,
    Hyperlink as RhwpHyperlink, PageHide as RhwpPageHide,
};
use rhwp::model::document::{
    Document as RhwpDocument, Section as RhwpSection, SectionDef as RhwpSectionDef,
};
use rhwp::model::header_footer::HeaderFooterApply as RhwpHeaderFooterApply;
use rhwp::model::image::{ImageEffect as RhwpImageEffect, Picture};
use rhwp::model::page::{
    ColumnDef as RhwpColumnDef, ColumnDirection as RhwpColumnDirection,
    ColumnType as RhwpColumnType,
};
use rhwp::model::paragraph::{
    CharShapeRef, FieldRange, NumberingRestart as RhwpNumberingRestart, Paragraph as RhwpParagraph,
};
use rhwp::model::shape::{
    Caption as RhwpCaption, CaptionDirection as RhwpCaptionDirection, ShapeObject,
};
use rhwp::model::style::{
    Alignment as RhwpAlignment, BorderFill as RhwpBorderFill, CharShape as RhwpCharShape,
    FillType as RhwpFillType, HeadType as RhwpHeadType, Numbering as RhwpNumbering,
    ParaShape as RhwpParaShape, UnderlineType as RhwpUnderlineType,
};
use rhwp::model::table::{Cell as RhwpCell, Table as RhwpTable};

use crate::hwpx::{self, HwpxTextFallbackSource, InputKind};
use crate::ir::{
    Block, Color, ConversionWarning, Document, Equation, EquationKind, HeaderFooter,
    HeaderFooterPlacement, Image, ImageResource, Inline, LengthPt, LengthPx, Link, ListInfo,
    ListKind, NamedParagraphStyle, NamedTextStyle, Note, NoteId, NoteKind, NoteStore, Paragraph,
    ParagraphRole, ParagraphStyle, ParagraphStyleId, Resource, ResourceId, ResourceStore, Section,
    Shape, ShapeKind, Spacing, StyleSheet, Table, TableCell, TableCellStyle, TableRow, TableStyle,
    TextRun, TextStyle, TextStyleId, UnknownInline, WarningCode,
};

/// Parse a source document with `rhwp` and bridge the resulting model into the
/// local `Document` IR. For `.hwpx`, section XML fallback remains available
/// when parsing fails or when the mapped body is structurally empty.
pub fn read_document(input_path: &Path) -> Result<Document, Box<dyn Error>> {
    let (input_kind, bytes) = hwpx::read_input_bytes(input_path)?;

    match rhwp::parse_document(&bytes) {
        Ok(parsed) => {
            let bridged = BridgeContext::new(&parsed).into_document();
            if document_has_blocks(&bridged) {
                Ok(bridged)
            } else if input_kind == InputKind::Hwpx {
                let empty_error = empty_document_error();
                fallback_to_hwpx_document(&bytes, &empty_error).map_err(Into::into)
            } else {
                Err(empty_document_error().into())
            }
        }
        Err(error) => {
            let rhwp_error = io::Error::new(
                io::ErrorKind::InvalidData,
                format!("rhwp 파싱 실패: {error}"),
            );

            if input_kind == InputKind::Hwpx {
                fallback_to_hwpx_document(&bytes, &rhwp_error).map_err(Into::into)
            } else {
                Err(rhwp_error.into())
            }
        }
    }
}

fn fallback_to_hwpx_document(bytes: &[u8], source_error: &io::Error) -> io::Result<Document> {
    let fallback = hwpx::read_document_fallback_from_archive(bytes)
        .map_err(|fallback_error| hwpx::combine_hwpx_errors(source_error, &fallback_error))?;

    Ok(document_from_hwpx_fallback(
        fallback.document,
        fallback.source,
    ))
}

fn document_from_hwpx_fallback(mut document: Document, source: HwpxTextFallbackSource) -> Document {
    let warning = match source {
        HwpxTextFallbackSource::SectionXml => ConversionWarning {
            code: WarningCode::Unknown,
            message: "Used HWPX section XML fallback. This recovers paragraph text, inline line breaks/tabs, sections, tables, captions, image resources, list metadata, links, fields/bookmarks, headers/footers, notes, equations, shapes, charts, unsupported-control placeholders, and some basic styles, but visual layout data may be missing.".to_string(),
        },
        HwpxTextFallbackSource::PreviewText => ConversionWarning {
            code: WarningCode::UsedHwpxPreviewFallback,
            message: "Used HWPX preview fallback. Preview/PrvText.txt only recovers plain text, so table, image, and style data may be missing.".to_string(),
        },
    };
    document.warnings.push(warning);
    document
}

fn document_has_blocks(document: &Document) -> bool {
    !document.notes.notes.is_empty()
        || document.sections.iter().any(|section| {
            !section.blocks.is_empty() || !section.headers.is_empty() || !section.footers.is_empty()
        })
}

fn empty_document_error() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        "rhwp document mapping produced no blocks",
    )
}

struct BridgeContext<'a> {
    source: &'a RhwpDocument,
    resources: ResourceStore,
    notes: NoteStore,
    warnings: Vec<ConversionWarning>,
    next_footnote_id: usize,
    next_endnote_id: usize,
}

impl<'a> BridgeContext<'a> {
    fn new(source: &'a RhwpDocument) -> Self {
        Self {
            source,
            resources: ResourceStore::default(),
            notes: NoteStore::default(),
            warnings: Vec::new(),
            next_footnote_id: 1,
            next_endnote_id: 1,
        }
    }

    fn into_document(mut self) -> Document {
        let sections = self
            .source
            .sections
            .iter()
            .map(|section| self.map_section(section))
            .collect();
        let styles = self.map_style_sheet();

        Document {
            ir_version: crate::ir::IR_VERSION,
            metadata: crate::ir::Metadata::default(),
            sections,
            resources: self.resources,
            styles,
            notes: self.notes,
            warnings: self.warnings,
        }
    }

    fn map_section(&mut self, section: &RhwpSection) -> Section {
        let mut blocks = Vec::new();
        let mut headers = Vec::new();
        let mut footers = Vec::new();
        let mut list_state = ListState::default();

        for paragraph in &section.paragraphs {
            self.append_blocks_from_paragraph(
                &mut blocks,
                paragraph,
                section.section_def.outline_numbering_id,
                &mut list_state,
            );
            self.collect_section_header_footers(paragraph, &mut headers, &mut footers);
        }

        Section {
            blocks,
            headers,
            footers,
        }
    }

    fn append_blocks_from_paragraph(
        &mut self,
        blocks: &mut Vec<Block>,
        paragraph: &RhwpParagraph,
        outline_numbering_id: u16,
        list_state: &mut ListState,
    ) {
        if let Some(mapped) = self.map_paragraph(paragraph, outline_numbering_id, list_state) {
            blocks.push(Block::Paragraph(mapped));
        }

        for control in &paragraph.controls {
            blocks.extend(self.map_control_blocks(control));
        }
    }

    fn map_paragraph(
        &mut self,
        paragraph: &RhwpParagraph,
        outline_numbering_id: u16,
        list_state: &mut ListState,
    ) -> Option<Paragraph> {
        let inlines = self.map_paragraph_inlines(paragraph);
        if inlines.is_empty() {
            return None;
        }

        Some(Paragraph {
            role: self.map_paragraph_role(paragraph),
            inlines,
            style: self.map_paragraph_style_by_id(paragraph.para_shape_id),
            style_ref: self.paragraph_style_ref(paragraph),
            list: self.map_list_info(paragraph, outline_numbering_id, list_state),
        })
    }

    fn map_paragraph_role(&self, paragraph: &RhwpParagraph) -> ParagraphRole {
        let Some(para_shape) = self.lookup_para_shape(paragraph.para_shape_id) else {
            return ParagraphRole::Body;
        };

        match para_shape.head_type {
            RhwpHeadType::Outline => ParagraphRole::Heading {
                level: para_shape.para_level.saturating_add(1).clamp(1, 6),
            },
            _ => ParagraphRole::Body,
        }
    }

    fn map_paragraph_inlines(&mut self, paragraph: &RhwpParagraph) -> Vec<Inline> {
        let chars: Vec<char> = paragraph.text.chars().collect();
        let segments = self.build_text_segments(paragraph, chars.len());

        // Build final inlines with best-effort placement of control inlines.
        self.build_inlines_with_control_placement(paragraph, &chars, &segments)
    }

    fn find_substring_char_index(&self, text: &str, substring: &str) -> Option<usize> {
        let byte_idx = text.find(substring)?;
        Some(text[..byte_idx].chars().count())
    }

    fn build_inlines_with_control_placement(
        &mut self,
        paragraph: &RhwpParagraph,
        chars: &[char],
        segments: &[TextSegment],
    ) -> Vec<Inline> {
        let link_ranges = self.collect_link_ranges(paragraph, chars, segments);
        let consumed_control_idxs = link_ranges
            .iter()
            .map(|range| range.control_idx)
            .collect::<std::collections::BTreeSet<usize>>();

        let mut insertions = Vec::new();

        for (index, control) in paragraph.controls.iter().enumerate() {
            if consumed_control_idxs.contains(&index) {
                continue;
            }

            match control {
                Control::Footnote(note) => {
                    let note_id =
                        self.store_note(NoteKind::Footnote, note.number, &note.paragraphs);
                    let inline = Inline::FootnoteRef { note_id };
                    // try to match the footnote number in paragraph text
                    let preferred = if note.number > 0 {
                        let token = note.number.to_string();
                        self.find_substring_char_index(&paragraph.text, &token)
                    } else {
                        None
                    };
                    insertions.push(InlineInsertion {
                        position: preferred,
                        control_idx: index,
                        inline,
                    });
                }
                Control::Endnote(note) => {
                    let note_id = self.store_note(NoteKind::Endnote, note.number, &note.paragraphs);
                    let inline = Inline::EndnoteRef { note_id };
                    let preferred = if note.number > 0 {
                        let token = note.number.to_string();
                        self.find_substring_char_index(&paragraph.text, &token)
                    } else {
                        None
                    };
                    insertions.push(InlineInsertion {
                        position: preferred,
                        control_idx: index,
                        inline,
                    });
                }
                Control::Hyperlink(link) => {
                    if let Some(mapped) = self.map_trailing_hyperlink(link) {
                        let preferred = non_empty_string(&link.text)
                            .and_then(|t| self.find_substring_char_index(&paragraph.text, &t));
                        insertions.push(InlineInsertion {
                            position: preferred,
                            control_idx: index,
                            inline: Inline::Link(mapped),
                        });
                    } else if let Some(inline) = self.map_hyperlink_fallback(link) {
                        self.add_warning_once(
                            "rhwp hyperlink control URL was not URL-like; hwp-convert preserved it as unknown inline fallback text.",
                        );
                        insertions.push(InlineInsertion {
                            position: None,
                            control_idx: index,
                            inline,
                        });
                    }
                }
                Control::Field(field) if field.field_type == RhwpFieldType::Hyperlink => {
                    if let Some(mapped) = self.map_field_hyperlink(field) {
                        let label = field
                            .guide_text()
                            .or_else(|| field.field_name())
                            .map(|s| s.to_string())
                            .unwrap_or_default();
                        let preferred = if !label.is_empty() {
                            self.find_substring_char_index(&paragraph.text, &label)
                        } else {
                            None
                        };
                        insertions.push(InlineInsertion {
                            position: preferred,
                            control_idx: index,
                            inline: Inline::Link(mapped),
                        });
                    } else if let Some(inline) = self.map_field_fallback(field) {
                        self.add_warning_once(
                            "rhwp hyperlink field command was not URL-like; hwp-convert preserved it as unknown inline fallback text.",
                        );
                        insertions.push(InlineInsertion {
                            position: None,
                            control_idx: index,
                            inline,
                        });
                    }
                }
                Control::Field(field) => {
                    if field.field_type == RhwpFieldType::ClickHere {
                        if let Some(inline) = self.map_click_here_field(field) {
                            // click-here fallback text may be present verbatim
                            let fallback_text = match &inline {
                                Inline::Unknown(u) => u.fallback_text.clone().unwrap_or_default(),
                                _ => String::new(),
                            };
                            let preferred = if !fallback_text.is_empty() {
                                self.find_substring_char_index(&paragraph.text, &fallback_text)
                            } else {
                                None
                            };
                            insertions.push(InlineInsertion {
                                position: preferred,
                                control_idx: index,
                                inline,
                            });
                        }
                    } else if let Some(inline) = self.map_field_fallback(field) {
                        let fallback_text = match &inline {
                            Inline::Unknown(u) => u.fallback_text.clone().unwrap_or_default(),
                            _ => String::new(),
                        };
                        let preferred = if !fallback_text.is_empty() {
                            self.find_substring_char_index(&paragraph.text, &fallback_text)
                        } else {
                            None
                        };
                        insertions.push(InlineInsertion {
                            position: preferred,
                            control_idx: index,
                            inline,
                        });
                    }
                }
                Control::Bookmark(bookmark) => {
                    if let Some(mapped) = self.map_bookmark_anchor(bookmark) {
                        // Prefer matching the explicit bookmark name when available
                        let preferred = non_empty_string(&bookmark.name).and_then(|name| {
                            self.find_substring_char_index(&paragraph.text, &name)
                        });
                        insertions.push(InlineInsertion {
                            position: preferred,
                            control_idx: index,
                            inline: mapped,
                        });
                    }
                }
                _ => {}
            }
        }

        // Sort insertions by position (None go last), then by original order.
        insertions.sort_by(|a, b| match (a.position, b.position) {
            (Some(pa), Some(pb)) => pa.cmp(&pb).then(a.control_idx.cmp(&b.control_idx)),
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => a.control_idx.cmp(&b.control_idx),
        });

        // Build final inlines by walking paragraph text and field ranges, inserting candidates.
        let mut final_inlines = Vec::new();
        let mut range_iter = link_ranges.into_iter().peekable();
        let mut insertion_iter = insertions.into_iter().peekable();

        let mut cursor = 0usize;
        let text_len = chars.len();

        while cursor < text_len {
            // If next insertion is at or before cursor, emit it now
            if let Some(insertion) = insertion_iter.peek()
                && insertion.position.is_some_and(|pos| pos <= cursor)
            {
                let insertion = insertion_iter
                    .next()
                    .expect("peeked insertion should exist");
                final_inlines.push(insertion.inline);
                continue;
            }

            // If next range starts at or before cursor, emit the link block
            if let Some(next_range) = range_iter.peek() {
                if next_range.start <= cursor && next_range.end > cursor {
                    let range = range_iter.next().expect("peeked range should exist");
                    final_inlines.push(Inline::Link(range.link));
                    cursor = range.end.min(text_len);
                    continue;
                }

                if next_range.end <= cursor {
                    range_iter.next();
                    continue;
                }
            }

            // Determine next boundary: next insertion pos or next range start or text end
            let next_insertion_pos = insertion_iter
                .peek()
                .and_then(|insertion| insertion.position)
                .unwrap_or(text_len);
            let next_range_start = range_iter
                .peek()
                .map(|range| range.start)
                .unwrap_or(text_len);
            let next_boundary = next_insertion_pos.min(next_range_start).min(text_len);

            if cursor < next_boundary {
                self.push_text_range_as_inlines(
                    &mut final_inlines,
                    chars,
                    segments,
                    cursor,
                    next_boundary,
                );
                cursor = next_boundary;
                continue;
            }

            // Fallback: neither insertion nor range advanced; break to avoid infinite loop
            break;
        }

        // After walking text, append any remaining insertions (those with None pos)
        for insertion in insertion_iter {
            final_inlines.push(insertion.inline);
        }

        if final_inlines
            .iter()
            .any(|i| matches!(i, Inline::FootnoteRef { .. } | Inline::EndnoteRef { .. }))
        {
            self.add_warning_once(
                "rhwp footnote/endnote controls do not expose exact inline positions, so note references were appended after paragraph text.",
            );
        }

        if final_inlines.iter().any(|i| matches!(i, Inline::Link(_))) {
            self.add_warning_once(
                "Some rhwp hyperlinks could not be placed at exact inline positions, so bridge fallback appended them after paragraph text when possible.",
            );
        }

        if final_inlines
            .iter()
            .any(|i| matches!(i, Inline::Unknown(_)))
        {
            self.add_warning_once(
                "Some rhwp click-here fields or other field controls could not be placed at exact inline positions, so bridge fallback appended their fallback text after paragraph text.",
            );
        }

        if final_inlines
            .iter()
            .any(|i| matches!(i, Inline::Anchor { .. }))
        {
            self.add_warning_once(
                "rhwp exposed bookmark controls; hwp-convert preserved bookmark names as anchor inlines when available.",
            );
        }

        final_inlines
    }

    fn collect_link_ranges(
        &self,
        paragraph: &RhwpParagraph,
        chars: &[char],
        segments: &[TextSegment],
    ) -> Vec<LinkRange> {
        let mut ranges = paragraph.field_ranges.clone();
        ranges.sort_by_key(|range| (range.start_char_idx, range.end_char_idx));

        let mut link_ranges = Vec::new();
        let mut cursor = 0usize;
        for range in ranges {
            if range.start_char_idx >= range.end_char_idx
                || range.end_char_idx > chars.len()
                || range.start_char_idx < cursor
            {
                continue;
            }

            let Some(link) = self.map_link_from_field_range(paragraph, &range, chars, segments)
            else {
                continue;
            };

            cursor = range.end_char_idx;
            link_ranges.push(LinkRange {
                start: range.start_char_idx,
                end: range.end_char_idx,
                control_idx: range.control_idx,
                link,
            });
        }

        link_ranges
    }

    fn build_text_segments(&self, paragraph: &RhwpParagraph, text_len: usize) -> Vec<TextSegment> {
        if text_len == 0 {
            return Vec::new();
        }

        let fallback_style_id = self
            .source
            .doc_info
            .styles
            .get(paragraph.style_id as usize)
            .map(|style| style.char_shape_id as u32);
        let fallback_style = fallback_style_id
            .and_then(|char_shape_id| self.lookup_char_shape(char_shape_id))
            .map(|char_shape| self.map_text_style(char_shape))
            .unwrap_or_default();
        let fallback_style_ref = self.text_style_ref(paragraph.style_id);

        let mut refs = paragraph.char_shapes.clone();
        refs.sort_by_key(|char_shape| char_shape.start_pos);

        if refs.is_empty() {
            return vec![TextSegment {
                start: 0,
                end: text_len,
                style: fallback_style,
                style_ref: fallback_style_ref,
            }];
        }

        if refs
            .first()
            .is_some_and(|char_shape| char_shape.start_pos > 0)
        {
            refs.insert(
                0,
                CharShapeRef {
                    start_pos: 0,
                    char_shape_id: fallback_style_id.unwrap_or(refs[0].char_shape_id),
                },
            );
        }

        let mut segments = Vec::new();

        for (index, char_shape_ref) in refs.iter().enumerate() {
            let next_start = refs
                .get(index + 1)
                .map(|next| next.start_pos)
                .unwrap_or(u32::MAX);
            let start =
                char_index_for_utf16_position(paragraph, char_shape_ref.start_pos, text_len);
            let end = char_index_for_utf16_position(paragraph, next_start, text_len);
            let style = self
                .lookup_char_shape(char_shape_ref.char_shape_id)
                .map(|char_shape| self.map_text_style(char_shape))
                .unwrap_or_else(|| fallback_style.clone());
            let style_ref = if fallback_style_id == Some(char_shape_ref.char_shape_id) {
                fallback_style_ref.clone()
            } else {
                None
            };

            segments.push(TextSegment {
                start,
                end,
                style,
                style_ref,
            });
        }

        segments
    }

    fn collect_section_header_footers(
        &mut self,
        paragraph: &RhwpParagraph,
        headers: &mut Vec<HeaderFooter>,
        footers: &mut Vec<HeaderFooter>,
    ) {
        for control in &paragraph.controls {
            match control {
                Control::Header(header) => {
                    headers.push(self.map_header_footer(header.apply_to, &header.paragraphs))
                }
                Control::Footer(footer) => {
                    footers.push(self.map_header_footer(footer.apply_to, &footer.paragraphs))
                }
                _ => {}
            }
        }
    }

    fn map_header_footer(
        &mut self,
        apply_to: RhwpHeaderFooterApply,
        paragraphs: &[RhwpParagraph],
    ) -> HeaderFooter {
        HeaderFooter {
            placement: map_header_footer_placement(apply_to),
            blocks: self.map_blocks_from_paragraphs(paragraphs, 0),
        }
    }

    fn map_blocks_from_paragraphs(
        &mut self,
        paragraphs: &[RhwpParagraph],
        outline_numbering_id: u16,
    ) -> Vec<Block> {
        let mut blocks = Vec::new();
        let mut list_state = ListState::default();

        for paragraph in paragraphs {
            self.append_blocks_from_paragraph(
                &mut blocks,
                paragraph,
                outline_numbering_id,
                &mut list_state,
            );
        }

        blocks
    }

    fn push_text_range_as_inlines(
        &self,
        inlines: &mut Vec<Inline>,
        chars: &[char],
        segments: &[TextSegment],
        start: usize,
        end: usize,
    ) {
        if start >= end || end > chars.len() {
            return;
        }

        for segment in segments {
            let segment_start = segment.start.max(start);
            let segment_end = segment.end.min(end);
            if segment_start >= segment_end {
                continue;
            }

            let text: String = chars[segment_start..segment_end].iter().collect();
            push_text_fragment(inlines, &text, &segment.style, segment.style_ref.as_ref());
        }
    }

    fn map_link_from_field_range(
        &self,
        paragraph: &RhwpParagraph,
        range: &FieldRange,
        chars: &[char],
        segments: &[TextSegment],
    ) -> Option<Link> {
        let control = paragraph.controls.get(range.control_idx)?;

        let url = match control {
            Control::Field(field) if field.field_type == RhwpFieldType::Hyperlink => {
                non_empty_url_like_string(&field.command)
            }
            Control::Hyperlink(link) => non_empty_url_like_string(&link.url),
            _ => None,
        }?;

        let mut link_inlines = Vec::new();
        self.push_text_range_as_inlines(
            &mut link_inlines,
            chars,
            segments,
            range.start_char_idx,
            range.end_char_idx,
        );

        if link_inlines.is_empty() {
            let fallback_label = chars[range.start_char_idx..range.end_char_idx]
                .iter()
                .collect::<String>();
            if !fallback_label.is_empty() {
                link_inlines.push(Inline::Text(TextRun {
                    text: fallback_label,
                    style: TextStyle::default(),
                    style_ref: None,
                }));
            }
        }

        // Determine a sensible title where available from the control
        let title = match control {
            Control::Field(field) if field.field_type == RhwpFieldType::Hyperlink => field
                .guide_text()
                .map(|s| s.to_string())
                .or_else(|| field.field_name().map(|s| s.to_string())),
            Control::Hyperlink(link) => non_empty_string(&link.text),
            _ => None,
        };

        Some(Link {
            url,
            title,
            inlines: link_inlines,
        })
    }

    fn map_trailing_hyperlink(&self, link: &RhwpHyperlink) -> Option<Link> {
        let url = non_empty_url_like_string(&link.url)?;
        let label = non_empty_string(&link.text).unwrap_or_else(|| url.clone());

        let title = non_empty_string(&link.text);

        Some(Link {
            url,
            title,
            inlines: vec![Inline::Text(TextRun {
                text: label,
                style: TextStyle::default(),
                style_ref: None,
            })],
        })
    }

    fn map_hyperlink_fallback(&self, link: &RhwpHyperlink) -> Option<Inline> {
        let fallback_text = first_non_empty_string([
            non_empty_string(&link.text),
            non_empty_string(&link.url),
        ])?;

        Some(Inline::Unknown(UnknownInline {
            kind: "hyperlink".to_string(),
            fallback_text: Some(format!("[hyperlink: {fallback_text}]")),
            message: Some(
                "rHWP hyperlink control was preserved as fallback text because its URL was not URL-like."
                    .to_string(),
            ),
            source: Some("rhwp".to_string()),
        }))
    }

    fn map_click_here_field(&self, field: &RhwpField) -> Option<Inline> {
        let fallback_text = field
            .guide_text()
            .or_else(|| field.field_name())
            .or_else(|| field.memo_text())
            .map(ToOwned::to_owned)
            .or_else(|| non_empty_string(&field.command))?;

        Some(Inline::Unknown(UnknownInline {
            kind: "field:clickhere".to_string(),
            fallback_text: Some(fallback_text),
            message: Some(
                "ClickHere field was preserved as fallback text because exact inline placement is unavailable."
                    .to_string(),
            ),
            source: Some("rhwp".to_string()),
        }))
    }

    fn map_field_hyperlink(&self, field: &RhwpField) -> Option<Link> {
        let url = non_empty_url_like_string(&field.command)?;
        let label = field
            .guide_text()
            .or_else(|| field.field_name())
            .map(|s| s.to_string())
            .unwrap_or_else(|| url.clone());

        let title = field
            .guide_text()
            .or_else(|| field.field_name())
            .map(|s| s.to_string());

        Some(Link {
            url,
            title,
            inlines: vec![Inline::Text(TextRun {
                text: label,
                style: TextStyle::default(),
                style_ref: None,
            })],
        })
    }

    fn map_field_fallback(&self, field: &RhwpField) -> Option<Inline> {
        if field.field_type == RhwpFieldType::ClickHere {
            return self.map_click_here_field(field);
        }

        let kind = field_type_warning_name(field.field_type);
        let command = non_empty_string(&field.command)?;

        Some(Inline::Unknown(UnknownInline {
            kind: kind.to_string(),
            fallback_text: Some(format!("[{kind}: {command}]")),
            message: Some(
                "rHWP field command was preserved as fallback text because hwp-convert does not yet semantically map this field type."
                    .to_string(),
            ),
            source: Some("rhwp".to_string()),
        }))
    }

    fn map_bookmark_anchor(&self, bookmark: &RhwpBookmark) -> Option<Inline> {
        let name = non_empty_string(&bookmark.name)?;

        Some(Inline::Anchor {
            id: crate::util::plain_text::sanitize_anchor_id(&name),
        })
    }

    fn store_note(&mut self, kind: NoteKind, number: u16, paragraphs: &[RhwpParagraph]) -> NoteId {
        let blocks = self.map_blocks_from_paragraphs(paragraphs, 0);

        loop {
            let note_id = self.next_note_id(kind.clone(), number);
            let note = Note {
                id: note_id.clone(),
                kind: kind.clone(),
                blocks: blocks.clone(),
            };

            if self.notes.insert_unique(note).is_ok() {
                return note_id;
            }
        }
    }

    fn next_note_id(&mut self, kind: NoteKind, number: u16) -> NoteId {
        let prefix = match kind {
            NoteKind::Footnote => "footnote",
            NoteKind::Endnote => "endnote",
        };

        if number > 0 {
            let candidate = NoteId(format!("{prefix}-{number}"));
            if self.notes.get(&candidate).is_none() {
                return candidate;
            }
        }

        let counter = match kind {
            NoteKind::Footnote => &mut self.next_footnote_id,
            NoteKind::Endnote => &mut self.next_endnote_id,
        };
        let note_id = NoteId(format!("{prefix}-{}", *counter));
        *counter += 1;
        note_id
    }

    fn add_warning_once(&mut self, message: &str) {
        if self
            .warnings
            .iter()
            .any(|warning| warning.message == message)
        {
            return;
        }

        self.warnings.push(ConversionWarning {
            code: WarningCode::Unknown,
            message: message.to_string(),
        });
    }

    fn map_list_info(
        &self,
        paragraph: &RhwpParagraph,
        outline_numbering_id: u16,
        list_state: &mut ListState,
    ) -> Option<ListInfo> {
        let para_shape = self.lookup_para_shape(paragraph.para_shape_id)?;
        let level = para_shape.para_level.min(6);

        match para_shape.head_type {
            RhwpHeadType::None => None,
            RhwpHeadType::Bullet => Some(ListInfo {
                kind: ListKind::Unordered,
                level,
                marker: bullet_marker(self.source, para_shape.numbering_id),
                number: None,
            }),
            RhwpHeadType::Number | RhwpHeadType::Outline => {
                let numbering_id = resolve_numbering_id(para_shape, outline_numbering_id);
                let numbering = numbering_id
                    .checked_sub(1)
                    .and_then(|index| self.source.doc_info.numberings.get(index as usize));

                Some(ListInfo {
                    kind: ListKind::Ordered,
                    level,
                    marker: None,
                    number: numbering_id.checked_sub(1).and_then(|_| {
                        list_state.advance(
                            numbering_id,
                            level,
                            paragraph.numbering_restart,
                            numbering,
                        )
                    }),
                })
            }
        }
    }

    fn map_control_blocks(&mut self, control: &Control) -> Vec<Block> {
        match control {
            Control::SectionDef(section_def) => {
                self.warn_unsupported_layout_control(
                    "section_def",
                    section_def_fallback_text(section_def),
                );
                Vec::new()
            }
            Control::ColumnDef(column_def) => {
                if column_def_has_layout_effect(column_def) {
                    self.warn_unsupported_layout_control(
                        "column_def",
                        column_def_fallback_text(column_def),
                    );
                }
                Vec::new()
            }
            Control::Table(table) => self.map_table_blocks(table),
            Control::Picture(picture) => vec![self.map_picture_block(picture)],
            Control::Equation(equation) => vec![Block::Equation(self.map_equation(equation))],
            Control::Shape(shape) => self.map_shape_blocks(shape),
            Control::Unknown(control) => {
                let kind = format!("control:{:#010x}", control.ctrl_id);
                self.add_warning_once(&format!(
                    "rhwp exposed unknown control `{kind}`; hwp-convert preserved an unknown block placeholder.",
                ));
                vec![Block::Unknown(crate::ir::UnknownBlock {
                    kind,
                    fallback_text: None,
                    message: Some("rhwp exposed this control as Unknown".to_string()),
                    source: Some("rhwp".to_string()),
                })]
            }
            Control::AutoNumber(number) => self
                .warn_unsupported_control_with_fallback(
                    "auto_number",
                    format!(
                        "[auto number: type={}, number={}, assigned={}]",
                        auto_number_type_name(number.number_type),
                        number.number,
                        number.assigned_number
                    ),
                )
                .into_iter()
                .collect(),
            Control::NewNumber(number) => self
                .warn_unsupported_control_with_fallback(
                    "new_number",
                    format!(
                        "[new number: type={}, number={}]",
                        auto_number_type_name(number.number_type),
                        number.number
                    ),
                )
                .into_iter()
                .collect(),
            Control::PageNumberPos(position) => self
                .warn_unsupported_control_with_fallback(
                    "page_number_position",
                    format!(
                        "[page number position: format={}, position={}]",
                        position.format, position.position
                    ),
                )
                .into_iter()
                .collect(),
            Control::Bookmark(_) => Vec::new(),
            Control::Ruby(ruby) => self
                .warn_unsupported_visible_control(
                    "ruby",
                    non_empty_string(&ruby.ruby_text)
                        .map(|text| format!("[ruby: {text}]"))
                        .unwrap_or_else(|| "[ruby]".to_string()),
                )
                .into_iter()
                .collect(),
            Control::CharOverlap(overlap) => {
                let chars = overlap.chars.iter().collect::<String>();
                self.warn_unsupported_visible_control(
                    "char_overlap",
                    non_empty_string(&chars)
                        .map(|text| format!("[char overlap: {text}]"))
                        .unwrap_or_else(|| "[char overlap]".to_string()),
                )
                .into_iter()
                .collect()
            }
            Control::PageHide(page_hide) => self
                .warn_unsupported_control_with_fallback(
                    "page_hide",
                    page_hide_fallback_text(page_hide),
                )
                .into_iter()
                .collect(),
            Control::HiddenComment(comment) => {
                self.map_hidden_comment_block(comment).into_iter().collect()
            }
            Control::Field(field) => {
                if field.field_type != RhwpFieldType::Hyperlink {
                    self.warn_unsupported_control(field_type_warning_name(field.field_type));
                }
                Vec::new()
            }
            Control::Form(form) => self
                .warn_unsupported_control_with_fallback("form", form_fallback_text(form))
                .into_iter()
                .collect(),
            _ => Vec::new(),
        }
    }

    fn warn_unsupported_layout_control(&mut self, kind: &str, summary: String) {
        self.add_warning_once(&format!(
            "rhwp exposed layout control `{kind}`; hwp-convert does not yet model it in Document IR. Preserved summary: {summary}"
        ));
    }

    fn warn_unsupported_control(&mut self, kind: &str) -> Option<Block> {
        self.add_warning_once(&format!(
            "rhwp exposed unsupported control `{kind}`; hwp-convert recorded this warning to avoid silent data loss."
        ));
        None
    }

    fn warn_unsupported_visible_control(
        &mut self,
        kind: &str,
        fallback_text: String,
    ) -> Option<Block> {
        self.add_warning_once(&format!(
            "rhwp exposed unsupported visible control `{kind}`; hwp-convert preserved fallback text as an unknown block."
        ));

        Some(Block::Unknown(crate::ir::UnknownBlock {
            kind: kind.to_string(),
            fallback_text: Some(fallback_text),
            message: Some(
                "Unsupported visible rHWP control preserved as fallback text.".to_string(),
            ),
            source: Some("rhwp".to_string()),
        }))
    }

    fn warn_unsupported_control_with_fallback(
        &mut self,
        kind: &str,
        fallback_text: String,
    ) -> Option<Block> {
        self.add_warning_once(&format!(
            "rhwp exposed unsupported control `{kind}`; hwp-convert preserved fallback text as an unknown block."
        ));

        Some(Block::Unknown(crate::ir::UnknownBlock {
            kind: kind.to_string(),
            fallback_text: Some(fallback_text),
            message: Some("Unsupported rHWP control preserved as fallback text.".to_string()),
            source: Some("rhwp".to_string()),
        }))
    }

    fn map_hidden_comment_block(&mut self, comment: &RhwpHiddenComment) -> Option<Block> {
        let blocks = self.map_blocks_from_paragraphs(&comment.paragraphs, 0);
        let content = crate::util::plain_text::blocks_to_plain_text(&blocks);
        let fallback_text = if content.is_empty() {
            "[hidden comment]".to_string()
        } else {
            format!("[hidden comment]\n{content}")
        };

        self.add_warning_once(
            "rhwp exposed hidden comment paragraphs; hwp-convert preserved them as unknown block fallback text.",
        );

        Some(Block::Unknown(crate::ir::UnknownBlock {
            kind: "hidden_comment".to_string(),
            fallback_text: Some(fallback_text),
            message: Some(
                "Hidden comment preserved as fallback text because Document IR does not yet model comments."
                    .to_string(),
            ),
            source: Some("rhwp".to_string()),
        }))
    }

    fn map_table(&mut self, table: &RhwpTable) -> Table {
        let mut rows = Vec::new();
        let row_count = table
            .cells
            .iter()
            .map(|cell| cell.row.saturating_add(1))
            .max()
            .unwrap_or(table.row_count)
            .max(table.row_count);

        for row_index in 0..row_count {
            let mut row_cells = table
                .cells
                .iter()
                .filter(|cell| cell.row == row_index)
                .collect::<Vec<_>>();
            row_cells.sort_by_key(|cell| cell.col);

            rows.push(TableRow {
                cells: row_cells
                    .into_iter()
                    .map(|cell| self.map_table_cell(cell))
                    .collect(),
            });
        }

        Table {
            rows,
            style: TableStyle {
                background_color: self.border_fill_background_color(table.border_fill_id),
            },
        }
    }

    fn map_table_blocks(&mut self, table: &RhwpTable) -> Vec<Block> {
        let table_block = Block::Table(self.map_table(table));
        let Some(caption) = table.caption.as_ref() else {
            return vec![table_block];
        };

        let mut caption_blocks = self.map_caption_blocks(caption);
        if caption_blocks.is_empty() {
            return vec![table_block];
        }

        self.add_warning_once(
            "rhwp exposed table captions; hwp-convert preserved them as adjacent caption blocks because Table IR does not yet model table captions.",
        );

        match caption.direction {
            RhwpCaptionDirection::Left | RhwpCaptionDirection::Top => {
                caption_blocks.push(table_block);
                caption_blocks
            }
            RhwpCaptionDirection::Right | RhwpCaptionDirection::Bottom => {
                let mut blocks = vec![table_block];
                blocks.extend(caption_blocks);
                blocks
            }
        }
    }

    fn map_caption_blocks(&mut self, caption: &RhwpCaption) -> Vec<Block> {
        let mut blocks = self.map_blocks_from_paragraphs(&caption.paragraphs, 0);
        for block in &mut blocks {
            if let Block::Paragraph(paragraph) = block {
                paragraph.role = ParagraphRole::Caption;
            }
        }
        blocks
    }

    fn map_table_cell(&mut self, cell: &RhwpCell) -> TableCell {
        let mut blocks = self.map_blocks_from_paragraphs(&cell.paragraphs, 0);
        if let Some(field_name) = cell.field_name.as_deref().and_then(non_empty_string) {
            blocks.insert(
                0,
                Block::Unknown(crate::ir::UnknownBlock {
                    kind: "table_cell_field".to_string(),
                    fallback_text: Some(format!("[cell field: {field_name}]")),
                    message: Some(
                        "Table cell field name preserved as fallback text because Document IR does not yet model cell fields."
                            .to_string(),
                    ),
                    source: Some("rhwp".to_string()),
                }),
            );
            self.add_warning_once(
                "rhwp exposed table cell field names; hwp-convert preserved them as unknown block fallback text.",
            );
        }

        TableCell {
            row_span: (cell.row_span as u32).max(1),
            col_span: (cell.col_span as u32).max(1),
            blocks,
            style: TableCellStyle {
                background_color: self.border_fill_background_color(cell.border_fill_id),
            },
        }
    }

    fn map_picture_block(&mut self, picture: &Picture) -> Block {
        self.warn_unsupported_picture_transform(picture);

        match self.ensure_image_resource(picture.image_attr.bin_data_id) {
            Some(resource_id) => Block::Image(Image {
                resource_id,
                alt: non_empty_string(&picture.common.description),
                caption: self.caption_plain_text(
                    picture.caption.as_ref().map(|caption| &caption.paragraphs),
                ),
                width: hwp_units_to_px_option(picture.common.width),
                height: hwp_units_to_px_option(picture.common.height),
            }),
            None => Block::Unknown(crate::ir::UnknownBlock {
                kind: "picture".to_string(),
                fallback_text: Some("[image]".to_string()),
                message: Some(format!(
                    "bin data {} was missing, so the image resource could not be loaded",
                    picture.image_attr.bin_data_id
                )),
                source: Some("rhwp".to_string()),
            }),
        }
    }

    fn warn_unsupported_picture_transform(&mut self, picture: &Picture) {
        let mut details = Vec::new();

        if !picture_crop_is_empty(picture) {
            details.push(format!(
                "crop={}/{}/{}/{}",
                picture.crop.left, picture.crop.top, picture.crop.right, picture.crop.bottom
            ));
        }
        if picture.image_attr.effect != RhwpImageEffect::RealPic
            || picture.image_attr.brightness != 0
            || picture.image_attr.contrast != 0
        {
            details.push(format!(
                "image_attr=effect:{},brightness:{},contrast:{}",
                image_effect_name(picture.image_attr.effect),
                picture.image_attr.brightness,
                picture.image_attr.contrast
            ));
        }
        if picture.border_width != 0
            || picture.border_color != 0
            || picture.border_opacity != 0
            || picture.padding.left != 0
            || picture.padding.right != 0
            || picture.padding.top != 0
            || picture.padding.bottom != 0
        {
            details.push(format!(
                "border_width={},border_color={:#08x},border_opacity={},padding={}/{}/{}/{}",
                picture.border_width,
                picture.border_color,
                picture.border_opacity,
                picture.padding.left,
                picture.padding.right,
                picture.padding.top,
                picture.padding.bottom
            ));
        }

        if details.is_empty() {
            return;
        }

        self.add_warning_once(&format!(
            "rhwp exposed picture visual transforms that Image IR does not yet model; hwp-convert preserved the original image bytes without applying {}.",
            details.join(", ")
        ));
    }

    fn map_equation(&self, equation: &RhwpEquation) -> Equation {
        let content = non_empty_string(&equation.script);
        Equation {
            kind: EquationKind::PlainText,
            fallback_text: content.clone().or_else(|| Some("[equation]".to_string())),
            content,
            resource_id: None,
        }
    }

    fn map_shape(&mut self, shape: &ShapeObject) -> Shape {
        let kind = match shape {
            ShapeObject::Line(_) => ShapeKind::Line,
            ShapeObject::Rectangle(_) => ShapeKind::Rectangle,
            ShapeObject::Ellipse(_) | ShapeObject::Arc(_) => ShapeKind::Ellipse,
            ShapeObject::Polygon(_) | ShapeObject::Curve(_) => ShapeKind::Polygon,
            ShapeObject::Group(_) | ShapeObject::Picture(_) => ShapeKind::Unknown,
        };
        let description = non_empty_string(&shape.common().description);
        let text_box_text = self.shape_text_box_text(shape);
        let caption_text = match shape.drawing().and_then(|drawing| drawing.caption.as_ref()) {
            Some(caption) => self.caption_plain_text(Some(&caption.paragraphs)),
            None => None,
        };
        let mut fallback_parts = Vec::new();
        if let Some(description) = &description {
            fallback_parts.push(description.clone());
        }
        if let Some(text) = &text_box_text {
            fallback_parts.push(text.clone());
        }
        if let Some(caption) = caption_text {
            fallback_parts.push(caption);
        }
        if text_box_text.is_some() {
            self.add_warning_once(
                "rhwp exposed shape text box paragraphs; hwp-convert folded them into shape fallback text.",
            );
        }

        Shape {
            kind,
            fallback_text: if fallback_parts.is_empty() {
                Some("[shape]".to_string())
            } else {
                Some(fallback_parts.join("\n"))
            },
            description,
        }
    }

    fn map_shape_blocks(&mut self, shape: &ShapeObject) -> Vec<Block> {
        match shape {
            ShapeObject::Picture(picture) => vec![self.map_picture_block(picture)],
            ShapeObject::Group(group) => {
                self.add_warning_once(
                    "rhwp exposed grouped shape children; hwp-convert expanded them into sequential blocks without preserving group layout.",
                );

                let mut blocks = Vec::new();
                if let Some(description) = non_empty_string(&group.common.description) {
                    blocks.push(Block::Shape(Shape {
                        kind: ShapeKind::Unknown,
                        fallback_text: Some(description.clone()),
                        description: Some(description),
                    }));
                }

                for child in &group.children {
                    blocks.extend(self.map_shape_blocks(child));
                }

                if blocks.is_empty() {
                    blocks.push(Block::Shape(self.map_shape(shape)));
                }

                blocks
            }
            _ => vec![Block::Shape(self.map_shape(shape))],
        }
    }

    fn shape_text_box_text(&mut self, shape: &ShapeObject) -> Option<String> {
        let text_box = shape.drawing()?.text_box.as_ref()?;
        let blocks = self.map_blocks_from_paragraphs(&text_box.paragraphs, 0);
        let text = crate::util::plain_text::blocks_to_plain_text(&blocks);
        non_empty_string(&text)
    }

    fn ensure_image_resource(&mut self, bin_data_id: u16) -> Option<ResourceId> {
        let resource_id = ResourceId(format!("image-{bin_data_id}"));
        if self.resources.get(&resource_id).is_some() {
            return Some(resource_id);
        }

        let bin_data = self.find_bin_data_content(bin_data_id)?;
        let extension = non_empty_string(&bin_data.extension);
        let media_type = extension
            .as_deref()
            .and_then(media_type_for_extension)
            .map(ToOwned::to_owned);

        self.resources
            .insert_unique(Resource::Image(ImageResource {
                id: resource_id.clone(),
                media_type,
                extension,
                bytes: bin_data.data.clone(),
            }))
            .ok()?;

        Some(resource_id)
    }

    fn find_bin_data_content(
        &self,
        bin_data_id: u16,
    ) -> Option<&rhwp::model::bin_data::BinDataContent> {
        let list_index = bin_data_id.checked_sub(1)? as usize;

        if let Some(bin_data) = self.source.doc_info.bin_data_list.get(list_index)
            && let Some(content) = self
                .source
                .bin_data_content
                .iter()
                .find(|content| content.id == bin_data.storage_id)
        {
            return Some(content);
        }

        self.source
            .bin_data_content
            .iter()
            .find(|content| content.id == bin_data_id)
            .or_else(|| self.source.bin_data_content.get(list_index))
    }

    fn map_style_sheet(&self) -> StyleSheet {
        let mut style_sheet = StyleSheet::default();

        for (index, style) in self.source.doc_info.styles.iter().enumerate() {
            style_sheet.text_styles.push(NamedTextStyle {
                id: TextStyleId(text_style_key(index)),
                name: style_name(style),
                style: self
                    .lookup_char_shape(style.char_shape_id as u32)
                    .map(|char_shape| self.map_text_style(char_shape))
                    .unwrap_or_default(),
            });

            style_sheet.paragraph_styles.push(NamedParagraphStyle {
                id: ParagraphStyleId(paragraph_style_key(index)),
                name: style_name(style),
                style: self.map_paragraph_style_by_id(style.para_shape_id),
            });
        }

        style_sheet
    }

    fn map_text_style(&self, char_shape: &RhwpCharShape) -> TextStyle {
        TextStyle {
            bold: char_shape.bold,
            italic: char_shape.italic,
            underline: char_shape.underline_type != RhwpUnderlineType::None,
            strike: char_shape.strikethrough,
            font_family: self.lookup_font_family(char_shape),
            font_size_pt: i32_hwp_units_to_pt_option(char_shape.base_size),
            color: color_ref_to_color_option(char_shape.text_color),
            background_color: color_ref_to_color_option(char_shape.shade_color),
        }
    }

    fn map_paragraph_style_by_id(&self, para_shape_id: u16) -> ParagraphStyle {
        self.lookup_para_shape(para_shape_id)
            .map(|para_shape| self.map_paragraph_style(para_shape))
            .unwrap_or_default()
    }

    fn map_paragraph_style(&self, para_shape: &RhwpParaShape) -> ParagraphStyle {
        ParagraphStyle {
            alignment: map_alignment(para_shape.alignment),
            spacing: Spacing {
                before_pt: i32_hwp_units_to_pt_option(para_shape.spacing_before),
                after_pt: i32_hwp_units_to_pt_option(para_shape.spacing_after),
                line_pt: match para_shape.line_spacing_type {
                    rhwp::model::style::LineSpacingType::Fixed
                    | rhwp::model::style::LineSpacingType::SpaceOnly
                    | rhwp::model::style::LineSpacingType::Minimum => {
                        i32_hwp_units_to_pt_option(para_shape.line_spacing_v2 as i32)
                            .or_else(|| i32_hwp_units_to_pt_option(para_shape.line_spacing))
                    }
                    rhwp::model::style::LineSpacingType::Percent => None,
                },
            },
            indent: crate::ir::Indent {
                left_pt: i32_hwp_units_to_pt_option(para_shape.margin_left),
                right_pt: i32_hwp_units_to_pt_option(para_shape.margin_right),
                first_line_pt: i32_hwp_units_to_pt_option(para_shape.indent),
            },
        }
    }

    fn paragraph_style_ref(&self, paragraph: &RhwpParagraph) -> Option<ParagraphStyleId> {
        self.source
            .doc_info
            .styles
            .get(paragraph.style_id as usize)
            .map(|_| ParagraphStyleId(paragraph_style_key(paragraph.style_id as usize)))
    }

    fn text_style_ref(&self, style_id: u8) -> Option<TextStyleId> {
        self.source
            .doc_info
            .styles
            .get(style_id as usize)
            .map(|_| TextStyleId(text_style_key(style_id as usize)))
    }

    fn lookup_char_shape(&self, char_shape_id: u32) -> Option<&RhwpCharShape> {
        self.source.doc_info.char_shapes.get(char_shape_id as usize)
    }

    fn lookup_para_shape(&self, para_shape_id: u16) -> Option<&RhwpParaShape> {
        self.source.doc_info.para_shapes.get(para_shape_id as usize)
    }

    fn lookup_font_family(&self, char_shape: &RhwpCharShape) -> Option<String> {
        for (language_index, font_id) in char_shape.font_ids.iter().enumerate() {
            let Some(group) = self.source.doc_info.font_faces.get(language_index) else {
                continue;
            };
            let Some(font) = group.get(*font_id as usize) else {
                continue;
            };
            if let Some(name) = non_empty_string(&font.name) {
                return Some(name);
            }
        }

        None
    }

    fn border_fill_background_color(&self, border_fill_id: u16) -> Option<Color> {
        self.lookup_border_fill(border_fill_id)
            .and_then(border_fill_background_color)
    }

    fn lookup_border_fill(&self, border_fill_id: u16) -> Option<&RhwpBorderFill> {
        if let Some(border_fill) = border_fill_id
            .checked_sub(1)
            .and_then(|index| self.source.doc_info.border_fills.get(index as usize))
        {
            return Some(border_fill);
        }

        self.source
            .doc_info
            .border_fills
            .get(border_fill_id as usize)
    }

    fn caption_plain_text(&mut self, paragraphs: Option<&Vec<RhwpParagraph>>) -> Option<String> {
        let paragraphs = paragraphs?;
        let blocks = self.map_blocks_from_paragraphs(paragraphs, 0);
        let text = crate::util::plain_text::blocks_to_plain_text(&blocks);

        non_empty_string(&text)
    }
}

#[derive(Clone)]
struct TextSegment {
    start: usize,
    end: usize,
    style: TextStyle,
    style_ref: Option<TextStyleId>,
}

struct LinkRange {
    start: usize,
    end: usize,
    control_idx: usize,
    link: Link,
}

struct InlineInsertion {
    position: Option<usize>,
    control_idx: usize,
    inline: Inline,
}

#[derive(Default)]
struct ListState {
    counters: BTreeMap<u16, [u32; 7]>,
}

impl ListState {
    fn advance(
        &mut self,
        numbering_id: u16,
        level: u8,
        restart: Option<RhwpNumberingRestart>,
        numbering: Option<&RhwpNumbering>,
    ) -> Option<u32> {
        if numbering_id == 0 {
            return None;
        }

        let level_index = (level as usize).min(6);
        let counters = self.counters.entry(numbering_id).or_insert([0; 7]);
        for counter in counters.iter_mut().skip(level_index + 1) {
            *counter = 0;
        }

        let default_start = numbering
            .map(|numbering| {
                let level_start = numbering.level_start_numbers[level_index];
                if level_start > 0 {
                    level_start
                } else if level_index == 0 && numbering.start_number > 0 {
                    numbering.start_number as u32
                } else {
                    1
                }
            })
            .unwrap_or(1);

        counters[level_index] = match restart {
            Some(RhwpNumberingRestart::NewStart(number)) => number.max(1),
            Some(RhwpNumberingRestart::ContinuePrevious) | None if counters[level_index] > 0 => {
                counters[level_index] + 1
            }
            Some(RhwpNumberingRestart::ContinuePrevious) | None => default_start,
        };

        Some(counters[level_index])
    }
}

fn push_text_fragment(
    inlines: &mut Vec<Inline>,
    text: &str,
    style: &TextStyle,
    style_ref: Option<&TextStyleId>,
) {
    let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
    let mut buffer = String::new();

    for ch in normalized.chars() {
        match ch {
            '\n' => {
                flush_text_run(inlines, &mut buffer, style, style_ref);
                inlines.push(Inline::LineBreak);
            }
            '\t' => {
                flush_text_run(inlines, &mut buffer, style, style_ref);
                inlines.push(Inline::Tab);
            }
            _ => buffer.push(ch),
        }
    }

    flush_text_run(inlines, &mut buffer, style, style_ref);
}

fn flush_text_run(
    inlines: &mut Vec<Inline>,
    buffer: &mut String,
    style: &TextStyle,
    style_ref: Option<&TextStyleId>,
) {
    if buffer.is_empty() {
        return;
    }

    let text = std::mem::take(buffer);
    inlines.push(Inline::Text(TextRun {
        text,
        style: style.clone(),
        style_ref: style_ref.cloned(),
    }));
}

fn char_index_for_utf16_position(
    paragraph: &RhwpParagraph,
    utf16_position: u32,
    text_len: usize,
) -> usize {
    if utf16_position == u32::MAX {
        return text_len;
    }

    if paragraph.char_offsets.is_empty() {
        return (utf16_position as usize).min(text_len);
    }

    paragraph
        .char_offsets
        .iter()
        .position(|offset| *offset >= utf16_position)
        .unwrap_or(text_len)
}

fn hwp_units_to_px_option(value: u32) -> Option<LengthPx> {
    if value == 0 {
        None
    } else {
        Some(LengthPx(value as f32 / 75.0))
    }
}

fn i32_hwp_units_to_pt_option(value: i32) -> Option<LengthPt> {
    if value == 0 {
        None
    } else {
        Some(LengthPt(value as f32 / 100.0))
    }
}

fn color_ref_to_color_option(color_ref: u32) -> Option<Color> {
    if color_ref == 0 {
        None
    } else {
        Some(Color {
            r: (color_ref & 0xFF) as u8,
            g: ((color_ref >> 8) & 0xFF) as u8,
            b: ((color_ref >> 16) & 0xFF) as u8,
            a: 255,
        })
    }
}

fn border_fill_background_color(border_fill: &RhwpBorderFill) -> Option<Color> {
    if border_fill.fill.fill_type != RhwpFillType::Solid {
        return None;
    }

    border_fill
        .fill
        .solid
        .as_ref()
        .and_then(|solid| color_ref_to_color_option(solid.background_color))
}

fn map_alignment(alignment: RhwpAlignment) -> Option<crate::ir::Alignment> {
    Some(match alignment {
        RhwpAlignment::Left => crate::ir::Alignment::Left,
        RhwpAlignment::Center => crate::ir::Alignment::Center,
        RhwpAlignment::Right => crate::ir::Alignment::Right,
        RhwpAlignment::Justify | RhwpAlignment::Distribute | RhwpAlignment::Split => {
            crate::ir::Alignment::Justify
        }
    })
}

fn map_header_footer_placement(apply_to: RhwpHeaderFooterApply) -> HeaderFooterPlacement {
    match apply_to {
        RhwpHeaderFooterApply::Both => HeaderFooterPlacement::Default,
        RhwpHeaderFooterApply::Odd => HeaderFooterPlacement::OddPage,
        RhwpHeaderFooterApply::Even => HeaderFooterPlacement::EvenPage,
    }
}

fn resolve_numbering_id(para_shape: &RhwpParaShape, outline_numbering_id: u16) -> u16 {
    if para_shape.numbering_id == 0 && para_shape.head_type == RhwpHeadType::Outline {
        outline_numbering_id
    } else {
        para_shape.numbering_id
    }
}

fn field_type_warning_name(field_type: RhwpFieldType) -> &'static str {
    match field_type {
        RhwpFieldType::Unknown => "field:unknown",
        RhwpFieldType::Date => "field:date",
        RhwpFieldType::DocDate => "field:docdate",
        RhwpFieldType::Path => "field:path",
        RhwpFieldType::Bookmark => "field:bookmark",
        RhwpFieldType::MailMerge => "field:mailmerge",
        RhwpFieldType::CrossRef => "field:crossref",
        RhwpFieldType::Formula => "field:formula",
        RhwpFieldType::ClickHere => "field:clickhere",
        RhwpFieldType::Summary => "field:summary",
        RhwpFieldType::UserInfo => "field:userinfo",
        RhwpFieldType::Hyperlink => "field:hyperlink",
        RhwpFieldType::Memo => "field:memo",
        RhwpFieldType::PrivateInfoSecurity => "field:private_info",
        RhwpFieldType::TableOfContents => "field:table_of_contents",
    }
}

fn auto_number_type_name(number_type: RhwpAutoNumberType) -> &'static str {
    match number_type {
        RhwpAutoNumberType::Page => "page",
        RhwpAutoNumberType::Footnote => "footnote",
        RhwpAutoNumberType::Endnote => "endnote",
        RhwpAutoNumberType::Picture => "picture",
        RhwpAutoNumberType::Table => "table",
        RhwpAutoNumberType::Equation => "equation",
    }
}

fn section_def_fallback_text(section_def: &RhwpSectionDef) -> String {
    let hidden = section_def_hidden_flags(section_def);
    let hidden = if hidden.is_empty() {
        "none".to_string()
    } else {
        hidden.join(",")
    };

    format!(
        "page_num={}, page_num_type={}, picture_num={}, table_num={}, equation_num={}, outline_numbering_id={}, text_direction={}, page_size={}x{}, landscape={}, margins={}/{}/{}/{}, hidden={}",
        section_def.page_num,
        section_def.page_num_type,
        section_def.picture_num,
        section_def.table_num,
        section_def.equation_num,
        section_def.outline_numbering_id,
        section_def.text_direction,
        section_def.page_def.width,
        section_def.page_def.height,
        section_def.page_def.landscape,
        section_def.page_def.margin_left,
        section_def.page_def.margin_right,
        section_def.page_def.margin_top,
        section_def.page_def.margin_bottom,
        hidden
    )
}

fn section_def_hidden_flags(section_def: &RhwpSectionDef) -> Vec<&'static str> {
    let mut flags = Vec::new();
    if section_def.hide_header {
        flags.push("header");
    }
    if section_def.hide_footer {
        flags.push("footer");
    }
    if section_def.hide_master_page {
        flags.push("master_page");
    }
    if section_def.hide_border {
        flags.push("border");
    }
    if section_def.hide_fill {
        flags.push("fill");
    }
    if section_def.hide_empty_line {
        flags.push("empty_line");
    }
    flags
}

fn column_def_fallback_text(column_def: &RhwpColumnDef) -> String {
    format!(
        "column_type={}, column_count={}, direction={}, same_width={}, spacing={}, widths={:?}, gaps={:?}, separator_type={}, separator_width={}, separator_color={:#08x}",
        column_type_name(column_def.column_type),
        column_def.column_count,
        column_direction_name(column_def.direction),
        column_def.same_width,
        column_def.spacing,
        column_def.widths,
        column_def.gaps,
        column_def.separator_type,
        column_def.separator_width,
        column_def.separator_color
    )
}

fn column_def_has_layout_effect(column_def: &RhwpColumnDef) -> bool {
    let multi_column = column_def.column_count > 1;

    multi_column
        || !column_def.widths.is_empty()
        || !column_def.gaps.is_empty()
        || column_def.separator_type != 0
        || column_def.separator_width != 0
        || column_def.separator_color != 0
}

fn column_type_name(column_type: RhwpColumnType) -> &'static str {
    match column_type {
        RhwpColumnType::Normal => "normal",
        RhwpColumnType::Distribute => "distribute",
        RhwpColumnType::Parallel => "parallel",
    }
}

fn column_direction_name(direction: RhwpColumnDirection) -> &'static str {
    match direction {
        RhwpColumnDirection::LeftToRight => "left_to_right",
        RhwpColumnDirection::RightToLeft => "right_to_left",
    }
}

fn picture_crop_is_empty(picture: &Picture) -> bool {
    picture.crop.left == 0
        && picture.crop.top == 0
        && picture.crop.right == 0
        && picture.crop.bottom == 0
}

fn image_effect_name(effect: RhwpImageEffect) -> &'static str {
    match effect {
        RhwpImageEffect::RealPic => "real_pic",
        RhwpImageEffect::GrayScale => "gray_scale",
        RhwpImageEffect::BlackWhite => "black_white",
        RhwpImageEffect::Pattern8x8 => "pattern_8x8",
    }
}

fn page_hide_fallback_text(page_hide: &RhwpPageHide) -> String {
    let mut flags = Vec::new();
    if page_hide.hide_header {
        flags.push("header");
    }
    if page_hide.hide_footer {
        flags.push("footer");
    }
    if page_hide.hide_master_page {
        flags.push("master_page");
    }
    if page_hide.hide_border {
        flags.push("border");
    }
    if page_hide.hide_fill {
        flags.push("fill");
    }
    if page_hide.hide_page_num {
        flags.push("page_num");
    }

    if flags.is_empty() {
        "[page hide: none]".to_string()
    } else {
        format!("[page hide: {}]", flags.join(","))
    }
}

fn form_fallback_text(form: &RhwpFormObject) -> String {
    let label = non_empty_string(&form.caption)
        .or_else(|| non_empty_string(&form.text))
        .or_else(|| non_empty_string(&form.name))
        .unwrap_or_else(|| form_type_name(form.form_type).to_string());

    format!(
        "[form: type={}, name={}, text={}, value={}, enabled={}, size={}x{}]",
        form_type_name(form.form_type),
        non_empty_string(&form.name).unwrap_or_else(|| "-".to_string()),
        label,
        form.value,
        form.enabled,
        form.width,
        form.height
    )
}

fn form_type_name(form_type: RhwpFormType) -> &'static str {
    match form_type {
        RhwpFormType::PushButton => "push_button",
        RhwpFormType::CheckBox => "check_box",
        RhwpFormType::ComboBox => "combo_box",
        RhwpFormType::RadioButton => "radio_button",
        RhwpFormType::Edit => "edit",
    }
}

fn bullet_marker(source: &RhwpDocument, bullet_id: u16) -> Option<String> {
    if bullet_id == 0 {
        return None;
    }

    let bullet = source.doc_info.bullets.get((bullet_id - 1) as usize)?;
    normalize_bullet_char(bullet.bullet_char).map(|ch| ch.to_string())
}

fn normalize_bullet_char(ch: char) -> Option<char> {
    if ch == '\u{FFFF}' {
        return None;
    }

    let code = ch as u32;
    if (0xE000..=0xF8FF).contains(&code) {
        return Some('•');
    }

    Some(ch)
}

fn media_type_for_extension(extension: &str) -> Option<&'static str> {
    match extension.to_ascii_lowercase().as_str() {
        "png" => Some("image/png"),
        "jpg" | "jpeg" => Some("image/jpeg"),
        "gif" => Some("image/gif"),
        "bmp" => Some("image/bmp"),
        "svg" => Some("image/svg+xml"),
        "webp" => Some("image/webp"),
        _ => None,
    }
}

fn non_empty_string(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn non_empty_url_like_string(value: &str) -> Option<String> {
    let value = non_empty_string(value)?;
    let lower = value.to_ascii_lowercase();
    if value.starts_with('#')
        || lower.starts_with("mailto:")
        || lower.starts_with("tel:")
        || lower.starts_with("www.")
        || lower.contains("://")
    {
        Some(value)
    } else {
        None
    }
}

fn first_non_empty_string(values: impl IntoIterator<Item = Option<String>>) -> Option<String> {
    values.into_iter().flatten().find(|value| !value.is_empty())
}

fn style_name(style: &rhwp::model::style::Style) -> Option<String> {
    non_empty_string(&style.local_name).or_else(|| non_empty_string(&style.english_name))
}

fn paragraph_style_key(index: usize) -> String {
    format!("paragraph-style-{index}")
}

fn text_style_key(index: usize) -> String {
    format!("text-style-{index}")
}

#[cfg(test)]
mod tests {
    use super::*;

    use rhwp::model::bin_data::{
        BinData, BinDataCompression, BinDataContent, BinDataStatus, BinDataType,
    };
    use rhwp::model::control::{
        AutoNumber as RhwpAutoNumber, AutoNumberType as RhwpAutoNumberType,
        Bookmark as RhwpBookmark, CharOverlap as RhwpCharOverlap, Field as RhwpField,
        FormObject as RhwpFormObject, FormType as RhwpFormType, HiddenComment as RhwpHiddenComment,
        NewNumber as RhwpNewNumber, PageHide as RhwpPageHide, PageNumberPos as RhwpPageNumberPos,
        Ruby as RhwpRuby, UnknownControl as RhwpUnknownControl,
    };
    use rhwp::model::document::{
        DocInfo, Document as RhwpDocument, Section as RhwpSection, SectionDef as RhwpSectionDef,
    };
    use rhwp::model::footnote::Footnote as RhwpFootnote;
    use rhwp::model::header_footer::{
        Footer as RhwpFooter, Header as RhwpHeader, HeaderFooterApply as RhwpHeaderFooterApply,
    };
    use rhwp::model::image::{
        CropInfo as RhwpCropInfo, ImageAttr, ImageEffect as RhwpImageEffect, Picture,
    };
    use rhwp::model::page::{
        ColumnDef as RhwpColumnDef, ColumnDirection as RhwpColumnDirection,
        ColumnType as RhwpColumnType,
    };
    use rhwp::model::paragraph::{
        CharShapeRef, FieldRange, NumberingRestart as RhwpNumberingRestart,
        Paragraph as RhwpParagraph,
    };
    use rhwp::model::shape::{
        Caption as RhwpCaption, CaptionDirection as RhwpCaptionDirection,
        DrawingObjAttr as RhwpDrawingObjAttr, GroupShape as RhwpGroupShape,
        RectangleShape as RhwpRectangleShape, TextBox as RhwpTextBox,
    };
    use rhwp::model::style::{
        Alignment as RhwpAlignment, BorderFill as RhwpBorderFill, Bullet as RhwpBullet,
        CharShape as RhwpCharShape, Fill, FillType, Font, HeadType as RhwpHeadType,
        ParaShape as RhwpParaShape, SolidFill, Style as RhwpStyle,
        UnderlineType as RhwpUnderlineType,
    };
    use rhwp::model::table::{Cell as RhwpCell, Table as RhwpTable};

    #[test]
    fn maps_table_control_into_table_block() {
        let cell = RhwpCell {
            row: 0,
            col: 0,
            row_span: 1,
            col_span: 1,
            paragraphs: vec![RhwpParagraph {
                text: "cell text".to_string(),
                ..Default::default()
            }],
            ..Default::default()
        };
        let table = RhwpTable {
            row_count: 1,
            col_count: 1,
            cells: vec![cell],
            ..Default::default()
        };
        let document = RhwpDocument {
            sections: vec![RhwpSection {
                paragraphs: vec![RhwpParagraph {
                    controls: vec![Control::Table(Box::new(table))],
                    ..Default::default()
                }],
                ..Default::default()
            }],
            ..Default::default()
        };

        let bridged = BridgeContext::new(&document).into_document();

        match &bridged.sections[0].blocks[0] {
            Block::Table(table) => {
                assert_eq!(table.rows.len(), 1);
                assert_eq!(table.rows[0].cells.len(), 1);
                match &table.rows[0].cells[0].blocks[0] {
                    Block::Paragraph(paragraph) => {
                        assert_eq!(paragraph.inlines.len(), 1);
                        match &paragraph.inlines[0] {
                            Inline::Text(run) => assert_eq!(run.text, "cell text"),
                            other => panic!("expected text inline, got {other:?}"),
                        }
                    }
                    other => panic!("expected paragraph block, got {other:?}"),
                }
            }
            other => panic!("expected table block, got {other:?}"),
        }
    }

    #[test]
    fn recovers_table_rows_from_cell_coordinates_when_row_count_is_missing() {
        let table = RhwpTable {
            row_count: 0,
            col_count: 1,
            cells: vec![RhwpCell {
                row: 1,
                col: 0,
                row_span: 1,
                col_span: 1,
                paragraphs: vec![RhwpParagraph {
                    text: "late row".to_string(),
                    ..Default::default()
                }],
                ..Default::default()
            }],
            ..Default::default()
        };
        let document = RhwpDocument {
            sections: vec![RhwpSection {
                paragraphs: vec![RhwpParagraph {
                    controls: vec![Control::Table(Box::new(table))],
                    ..Default::default()
                }],
                ..Default::default()
            }],
            ..Default::default()
        };

        let bridged = BridgeContext::new(&document).into_document();

        match &bridged.sections[0].blocks[0] {
            Block::Table(table) => {
                assert_eq!(table.rows.len(), 2);
                assert!(table.rows[0].cells.is_empty());
                assert_eq!(table.rows[1].cells.len(), 1);
            }
            other => panic!("expected table block, got {other:?}"),
        }
    }

    #[test]
    fn preserves_table_cell_field_name_as_unknown_block() {
        let cell = RhwpCell {
            row: 0,
            col: 0,
            row_span: 1,
            col_span: 1,
            field_name: Some("amount".to_string()),
            paragraphs: vec![RhwpParagraph {
                text: "1000".to_string(),
                ..Default::default()
            }],
            ..Default::default()
        };
        let table = RhwpTable {
            row_count: 1,
            col_count: 1,
            cells: vec![cell],
            ..Default::default()
        };
        let document = RhwpDocument {
            sections: vec![RhwpSection {
                paragraphs: vec![RhwpParagraph {
                    controls: vec![Control::Table(Box::new(table))],
                    ..Default::default()
                }],
                ..Default::default()
            }],
            ..Default::default()
        };

        let bridged = BridgeContext::new(&document).into_document();

        match &bridged.sections[0].blocks[0] {
            Block::Table(table) => {
                let cell = &table.rows[0].cells[0];
                assert!(
                    matches!(&cell.blocks[0], Block::Unknown(unknown) if unknown.kind == "table_cell_field" && unknown.fallback_text.as_deref() == Some("[cell field: amount]"))
                );
                assert!(matches!(&cell.blocks[1], Block::Paragraph(_)));
            }
            other => panic!("expected table block, got {other:?}"),
        }
        assert!(
            bridged
                .warnings
                .iter()
                .any(|warning| { warning.message.contains("table cell field names") })
        );
    }

    #[test]
    fn preserves_table_caption_as_adjacent_caption_block() {
        let cell = RhwpCell {
            row: 0,
            col: 0,
            row_span: 1,
            col_span: 1,
            paragraphs: vec![RhwpParagraph {
                text: "cell".to_string(),
                ..Default::default()
            }],
            ..Default::default()
        };
        let table = RhwpTable {
            row_count: 1,
            col_count: 1,
            cells: vec![cell],
            caption: Some(RhwpCaption {
                direction: RhwpCaptionDirection::Bottom,
                paragraphs: vec![RhwpParagraph {
                    text: "Table caption".to_string(),
                    ..Default::default()
                }],
                ..Default::default()
            }),
            ..Default::default()
        };
        let document = RhwpDocument {
            sections: vec![RhwpSection {
                paragraphs: vec![RhwpParagraph {
                    controls: vec![Control::Table(Box::new(table))],
                    ..Default::default()
                }],
                ..Default::default()
            }],
            ..Default::default()
        };

        let bridged = BridgeContext::new(&document).into_document();
        let blocks = &bridged.sections[0].blocks;

        assert_eq!(blocks.len(), 2);
        assert!(matches!(&blocks[0], Block::Table(_)));
        match &blocks[1] {
            Block::Paragraph(paragraph) => {
                assert_eq!(paragraph.role, ParagraphRole::Caption);
                assert!(
                    matches!(&paragraph.inlines[0], Inline::Text(run) if run.text == "Table caption")
                );
            }
            other => panic!("expected caption paragraph block, got {other:?}"),
        }
        assert!(
            bridged
                .warnings
                .iter()
                .any(|warning| { warning.message.contains("table captions") })
        );
    }

    #[test]
    fn preserves_shape_text_box_in_fallback_text() {
        let shape = ShapeObject::Rectangle(RhwpRectangleShape {
            common: rhwp::model::shape::CommonObjAttr {
                description: "callout".to_string(),
                ..Default::default()
            },
            drawing: RhwpDrawingObjAttr {
                text_box: Some(RhwpTextBox {
                    paragraphs: vec![RhwpParagraph {
                        text: "shape text".to_string(),
                        ..Default::default()
                    }],
                    ..Default::default()
                }),
                ..Default::default()
            },
            ..Default::default()
        });
        let document = RhwpDocument {
            sections: vec![RhwpSection {
                paragraphs: vec![RhwpParagraph {
                    controls: vec![Control::Shape(Box::new(shape))],
                    ..Default::default()
                }],
                ..Default::default()
            }],
            ..Default::default()
        };

        let bridged = BridgeContext::new(&document).into_document();

        match &bridged.sections[0].blocks[0] {
            Block::Shape(shape) => {
                assert_eq!(shape.kind, ShapeKind::Rectangle);
                assert_eq!(shape.description.as_deref(), Some("callout"));
                assert_eq!(shape.fallback_text.as_deref(), Some("callout\nshape text"));
            }
            other => panic!("expected shape block, got {other:?}"),
        }
        assert!(
            bridged
                .warnings
                .iter()
                .any(|warning| { warning.message.contains("shape text box paragraphs") })
        );
    }

    #[test]
    fn maps_picture_control_into_image_block_and_resource_store() {
        let picture = Picture {
            image_attr: ImageAttr {
                bin_data_id: 7,
                ..Default::default()
            },
            common: rhwp::model::shape::CommonObjAttr {
                width: 7500,
                height: 3750,
                description: "logo".to_string(),
                ..Default::default()
            },
            ..Default::default()
        };
        let document = RhwpDocument {
            sections: vec![RhwpSection {
                paragraphs: vec![RhwpParagraph {
                    controls: vec![Control::Picture(Box::new(picture))],
                    ..Default::default()
                }],
                ..Default::default()
            }],
            bin_data_content: vec![BinDataContent {
                id: 7,
                data: vec![137, 80, 78, 71],
                extension: "png".to_string(),
            }],
            ..Default::default()
        };

        let bridged = BridgeContext::new(&document).into_document();

        match &bridged.sections[0].blocks[0] {
            Block::Image(image) => {
                assert_eq!(image.resource_id.as_str(), "image-7");
                assert_eq!(image.alt.as_deref(), Some("logo"));
                assert_eq!(image.width, Some(LengthPx(100.0)));
                assert_eq!(image.height, Some(LengthPx(50.0)));
            }
            other => panic!("expected image block, got {other:?}"),
        }

        match bridged.resources.entries.first() {
            Some(Resource::Image(resource)) => {
                assert_eq!(resource.id.as_str(), "image-7");
                assert_eq!(resource.extension.as_deref(), Some("png"));
                assert_eq!(resource.media_type.as_deref(), Some("image/png"));
            }
            other => panic!("expected image resource, got {other:?}"),
        }
    }

    #[test]
    fn preserves_picture_caption_field_fallback_text() {
        let picture = Picture {
            image_attr: ImageAttr {
                bin_data_id: 7,
                ..Default::default()
            },
            caption: Some(RhwpCaption {
                paragraphs: vec![RhwpParagraph {
                    controls: vec![Control::Field(RhwpField {
                        field_type: RhwpFieldType::ClickHere,
                        command: RhwpField::build_clickhere_command("caption field", "", ""),
                        ..Default::default()
                    })],
                    ..Default::default()
                }],
                ..Default::default()
            }),
            ..Default::default()
        };
        let document = RhwpDocument {
            sections: vec![RhwpSection {
                paragraphs: vec![RhwpParagraph {
                    controls: vec![Control::Picture(Box::new(picture))],
                    ..Default::default()
                }],
                ..Default::default()
            }],
            bin_data_content: vec![BinDataContent {
                id: 7,
                data: vec![137, 80, 78, 71],
                extension: "png".to_string(),
            }],
            ..Default::default()
        };

        let bridged = BridgeContext::new(&document).into_document();

        match &bridged.sections[0].blocks[0] {
            Block::Image(image) => {
                assert_eq!(image.caption.as_deref(), Some("caption field"));
            }
            other => panic!("expected image block, got {other:?}"),
        }
    }

    #[test]
    fn warns_when_picture_visual_transforms_are_not_modeled() {
        let picture = Picture {
            image_attr: ImageAttr {
                bin_data_id: 7,
                brightness: 10,
                contrast: -5,
                effect: RhwpImageEffect::GrayScale,
            },
            crop: RhwpCropInfo {
                left: 1,
                top: 2,
                right: 3,
                bottom: 4,
            },
            border_width: 5,
            border_color: 0x00112233,
            border_opacity: 128,
            padding: rhwp::model::Padding {
                left: 10,
                right: 20,
                top: 30,
                bottom: 40,
            },
            ..Default::default()
        };
        let document = RhwpDocument {
            sections: vec![RhwpSection {
                paragraphs: vec![RhwpParagraph {
                    controls: vec![Control::Picture(Box::new(picture))],
                    ..Default::default()
                }],
                ..Default::default()
            }],
            bin_data_content: vec![BinDataContent {
                id: 7,
                data: vec![137, 80, 78, 71],
                extension: "png".to_string(),
            }],
            ..Default::default()
        };

        let bridged = BridgeContext::new(&document).into_document();

        assert!(matches!(&bridged.sections[0].blocks[0], Block::Image(_)));
        assert!(bridged.warnings.iter().any(|warning| {
            warning.message.contains("picture visual transforms")
                && warning.message.contains("crop=1/2/3/4")
                && warning.message.contains("effect:gray_scale")
                && warning.message.contains("padding=10/20/30/40")
        }));
    }

    #[test]
    fn maps_shape_picture_into_image_block_and_resource_store() {
        let picture = Picture {
            image_attr: ImageAttr {
                bin_data_id: 9,
                ..Default::default()
            },
            common: rhwp::model::shape::CommonObjAttr {
                width: 15000,
                height: 7500,
                description: "nested logo".to_string(),
                ..Default::default()
            },
            ..Default::default()
        };
        let document = RhwpDocument {
            sections: vec![RhwpSection {
                paragraphs: vec![RhwpParagraph {
                    controls: vec![Control::Shape(Box::new(ShapeObject::Picture(Box::new(
                        picture,
                    ))))],
                    ..Default::default()
                }],
                ..Default::default()
            }],
            bin_data_content: vec![BinDataContent {
                id: 9,
                data: vec![137, 80, 78, 71],
                extension: "png".to_string(),
            }],
            ..Default::default()
        };

        let bridged = BridgeContext::new(&document).into_document();

        match &bridged.sections[0].blocks[0] {
            Block::Image(image) => {
                assert_eq!(image.resource_id.as_str(), "image-9");
                assert_eq!(image.alt.as_deref(), Some("nested logo"));
                assert_eq!(image.width, Some(LengthPx(200.0)));
                assert_eq!(image.height, Some(LengthPx(100.0)));
            }
            other => panic!("expected image block, got {other:?}"),
        }
    }

    #[test]
    fn expands_group_shape_children_into_sequential_blocks() {
        let rectangle = ShapeObject::Rectangle(RhwpRectangleShape {
            common: rhwp::model::shape::CommonObjAttr {
                description: "group rect".to_string(),
                ..Default::default()
            },
            ..Default::default()
        });
        let picture = ShapeObject::Picture(Box::new(Picture {
            image_attr: ImageAttr {
                bin_data_id: 11,
                ..Default::default()
            },
            common: rhwp::model::shape::CommonObjAttr {
                description: "group image".to_string(),
                ..Default::default()
            },
            ..Default::default()
        }));
        let group = ShapeObject::Group(RhwpGroupShape {
            children: vec![rectangle, picture],
            ..Default::default()
        });
        let document = RhwpDocument {
            sections: vec![RhwpSection {
                paragraphs: vec![RhwpParagraph {
                    controls: vec![Control::Shape(Box::new(group))],
                    ..Default::default()
                }],
                ..Default::default()
            }],
            bin_data_content: vec![BinDataContent {
                id: 11,
                data: vec![137, 80, 78, 71],
                extension: "png".to_string(),
            }],
            ..Default::default()
        };

        let bridged = BridgeContext::new(&document).into_document();
        let blocks = &bridged.sections[0].blocks;

        assert_eq!(blocks.len(), 2);
        assert!(
            matches!(&blocks[0], Block::Shape(shape) if shape.kind == ShapeKind::Rectangle && shape.fallback_text.as_deref() == Some("group rect"))
        );
        assert!(
            matches!(&blocks[1], Block::Image(image) if image.resource_id.as_str() == "image-11" && image.alt.as_deref() == Some("group image"))
        );
        assert!(
            bridged
                .warnings
                .iter()
                .any(|warning| { warning.message.contains("grouped shape children") })
        );
    }

    #[test]
    fn maps_picture_bin_data_id_through_doc_info_list_index() {
        let picture = Picture {
            image_attr: ImageAttr {
                bin_data_id: 1,
                ..Default::default()
            },
            common: rhwp::model::shape::CommonObjAttr {
                width: 7500,
                height: 3750,
                description: "indexed image".to_string(),
                ..Default::default()
            },
            ..Default::default()
        };
        let document = RhwpDocument {
            doc_info: DocInfo {
                bin_data_list: vec![BinData {
                    attr: 0x0101,
                    data_type: BinDataType::Embedding,
                    compression: BinDataCompression::Default,
                    status: BinDataStatus::Success,
                    storage_id: 3,
                    extension: Some("png".to_string()),
                    ..Default::default()
                }],
                ..Default::default()
            },
            sections: vec![RhwpSection {
                paragraphs: vec![RhwpParagraph {
                    controls: vec![Control::Picture(Box::new(picture))],
                    ..Default::default()
                }],
                ..Default::default()
            }],
            bin_data_content: vec![BinDataContent {
                id: 3,
                data: vec![137, 80, 78, 71],
                extension: "png".to_string(),
            }],
            ..Default::default()
        };

        let bridged = BridgeContext::new(&document).into_document();

        match &bridged.sections[0].blocks[0] {
            Block::Image(image) => {
                assert_eq!(image.resource_id.as_str(), "image-1");
                assert_eq!(image.alt.as_deref(), Some("indexed image"));
            }
            other => panic!("expected image block, got {other:?}"),
        }

        match bridged.resources.entries.first() {
            Some(Resource::Image(resource)) => {
                assert_eq!(resource.id.as_str(), "image-1");
                assert_eq!(resource.bytes, vec![137, 80, 78, 71]);
            }
            other => panic!("expected image resource, got {other:?}"),
        }
    }

    #[test]
    fn maps_text_and_paragraph_styles_from_rhwp_shapes() {
        let document = RhwpDocument {
            doc_info: DocInfo {
                font_faces: vec![vec![Font {
                    name: "Noto Sans KR".to_string(),
                    ..Default::default()
                }]],
                char_shapes: vec![RhwpCharShape {
                    bold: true,
                    italic: true,
                    underline_type: RhwpUnderlineType::Bottom,
                    strikethrough: true,
                    base_size: 1200,
                    text_color: 0x00010203,
                    shade_color: 0x00040506,
                    ..Default::default()
                }],
                para_shapes: vec![RhwpParaShape {
                    alignment: RhwpAlignment::Center,
                    margin_left: 300,
                    margin_right: 200,
                    indent: 100,
                    spacing_before: 400,
                    spacing_after: 500,
                    line_spacing_type: rhwp::model::style::LineSpacingType::Fixed,
                    line_spacing_v2: 600,
                    ..Default::default()
                }],
                styles: vec![RhwpStyle {
                    local_name: "body".to_string(),
                    para_shape_id: 0,
                    char_shape_id: 0,
                    ..Default::default()
                }],
                ..Default::default()
            },
            sections: vec![RhwpSection {
                paragraphs: vec![RhwpParagraph {
                    text: "styled".to_string(),
                    para_shape_id: 0,
                    style_id: 0,
                    char_offsets: vec![0, 1, 2, 3, 4, 5],
                    char_shapes: vec![CharShapeRef {
                        start_pos: 0,
                        char_shape_id: 0,
                    }],
                    ..Default::default()
                }],
                ..Default::default()
            }],
            ..Default::default()
        };

        let bridged = BridgeContext::new(&document).into_document();

        match &bridged.sections[0].blocks[0] {
            Block::Paragraph(paragraph) => {
                assert_eq!(paragraph.role, ParagraphRole::Body);
                assert_eq!(
                    paragraph.style.alignment,
                    Some(crate::ir::Alignment::Center)
                );
                assert_eq!(paragraph.style.spacing.before_pt, Some(LengthPt(4.0)));
                assert_eq!(paragraph.style.indent.left_pt, Some(LengthPt(3.0)));
                assert_eq!(
                    paragraph.style_ref,
                    Some(ParagraphStyleId("paragraph-style-0".to_string()))
                );

                match &paragraph.inlines[0] {
                    Inline::Text(run) => {
                        assert_eq!(run.text, "styled");
                        assert!(run.style.bold);
                        assert!(run.style.italic);
                        assert!(run.style.underline);
                        assert!(run.style.strike);
                        assert_eq!(run.style.font_family.as_deref(), Some("Noto Sans KR"));
                        assert_eq!(run.style.font_size_pt, Some(LengthPt(12.0)));
                        assert_eq!(
                            run.style.color,
                            Some(Color {
                                r: 3,
                                g: 2,
                                b: 1,
                                a: 255,
                            })
                        );
                        assert_eq!(
                            run.style.background_color,
                            Some(Color {
                                r: 6,
                                g: 5,
                                b: 4,
                                a: 255,
                            })
                        );
                    }
                    other => panic!("expected text inline, got {other:?}"),
                }
            }
            other => panic!("expected paragraph block, got {other:?}"),
        }

        assert_eq!(bridged.styles.text_styles.len(), 1);
        assert_eq!(bridged.styles.paragraph_styles.len(), 1);
    }

    #[test]
    fn maps_table_background_color_from_border_fill() {
        let document = RhwpDocument {
            doc_info: DocInfo {
                border_fills: vec![RhwpBorderFill {
                    fill: Fill {
                        fill_type: FillType::Solid,
                        solid: Some(SolidFill {
                            background_color: 0x0000FF00,
                            ..Default::default()
                        }),
                        ..Default::default()
                    },
                    ..Default::default()
                }],
                ..Default::default()
            },
            sections: vec![RhwpSection {
                paragraphs: vec![RhwpParagraph {
                    controls: vec![Control::Table(Box::new(RhwpTable {
                        row_count: 1,
                        col_count: 1,
                        border_fill_id: 0,
                        cells: vec![RhwpCell {
                            row: 0,
                            col: 0,
                            border_fill_id: 0,
                            ..Default::default()
                        }],
                        ..Default::default()
                    }))],
                    ..Default::default()
                }],
                ..Default::default()
            }],
            ..Default::default()
        };

        let bridged = BridgeContext::new(&document).into_document();

        match &bridged.sections[0].blocks[0] {
            Block::Table(table) => {
                assert_eq!(
                    table.style.background_color,
                    Some(Color {
                        r: 0,
                        g: 255,
                        b: 0,
                        a: 255,
                    })
                );
                assert_eq!(
                    table.rows[0].cells[0].style.background_color,
                    Some(Color {
                        r: 0,
                        g: 255,
                        b: 0,
                        a: 255,
                    })
                );
            }
            other => panic!("expected table block, got {other:?}"),
        }
    }

    #[test]
    fn maps_border_fill_ids_as_one_based_when_present() {
        let document = RhwpDocument {
            doc_info: DocInfo {
                border_fills: vec![
                    RhwpBorderFill {
                        fill: Fill {
                            fill_type: FillType::Solid,
                            solid: Some(SolidFill {
                                background_color: 0x000000FF,
                                ..Default::default()
                            }),
                            ..Default::default()
                        },
                        ..Default::default()
                    },
                    RhwpBorderFill {
                        fill: Fill {
                            fill_type: FillType::Solid,
                            solid: Some(SolidFill {
                                background_color: 0x0000FF00,
                                ..Default::default()
                            }),
                            ..Default::default()
                        },
                        ..Default::default()
                    },
                ],
                ..Default::default()
            },
            sections: vec![RhwpSection {
                paragraphs: vec![RhwpParagraph {
                    controls: vec![Control::Table(Box::new(RhwpTable {
                        row_count: 1,
                        col_count: 1,
                        border_fill_id: 1,
                        cells: vec![RhwpCell {
                            row: 0,
                            col: 0,
                            border_fill_id: 1,
                            ..Default::default()
                        }],
                        ..Default::default()
                    }))],
                    ..Default::default()
                }],
                ..Default::default()
            }],
            ..Default::default()
        };

        let bridged = BridgeContext::new(&document).into_document();

        match &bridged.sections[0].blocks[0] {
            Block::Table(table) => {
                let expected = Some(Color {
                    r: 255,
                    g: 0,
                    b: 0,
                    a: 255,
                });
                assert_eq!(table.style.background_color, expected);
                assert_eq!(table.rows[0].cells[0].style.background_color, expected);
            }
            other => panic!("expected table block, got {other:?}"),
        }
    }

    #[test]
    fn maps_hyperlink_field_ranges_into_link_inlines() {
        let document = RhwpDocument {
            sections: vec![RhwpSection {
                paragraphs: vec![RhwpParagraph {
                    text: "Visit Example now".to_string(),
                    field_ranges: vec![FieldRange {
                        start_char_idx: 6,
                        end_char_idx: 13,
                        control_idx: 0,
                    }],
                    controls: vec![Control::Field(RhwpField {
                        field_type: RhwpFieldType::Hyperlink,
                        command: "https://example.com".to_string(),
                        ..Default::default()
                    })],
                    ..Default::default()
                }],
                ..Default::default()
            }],
            ..Default::default()
        };

        let bridged = BridgeContext::new(&document).into_document();

        match &bridged.sections[0].blocks[0] {
            Block::Paragraph(paragraph) => {
                assert_eq!(paragraph.inlines.len(), 3);
                match &paragraph.inlines[1] {
                    Inline::Link(link) => {
                        assert_eq!(link.url, "https://example.com");
                        assert_eq!(link.inlines.len(), 1);
                        match &link.inlines[0] {
                            Inline::Text(run) => assert_eq!(run.text, "Example"),
                            other => panic!("expected text run in link, got {other:?}"),
                        }
                    }
                    other => panic!("expected link inline, got {other:?}"),
                }
            }
            other => panic!("expected paragraph block, got {other:?}"),
        }
    }

    #[test]
    fn preserves_non_url_hyperlink_field_command_as_unknown_inline() {
        let document = RhwpDocument {
            sections: vec![RhwpSection {
                paragraphs: vec![RhwpParagraph {
                    text: "Visit Example now".to_string(),
                    field_ranges: vec![FieldRange {
                        start_char_idx: 6,
                        end_char_idx: 13,
                        control_idx: 0,
                    }],
                    controls: vec![Control::Field(RhwpField {
                        field_type: RhwpFieldType::Hyperlink,
                        command: "not a url".to_string(),
                        ..Default::default()
                    })],
                    ..Default::default()
                }],
                ..Default::default()
            }],
            ..Default::default()
        };

        let bridged = BridgeContext::new(&document).into_document();

        match &bridged.sections[0].blocks[0] {
            Block::Paragraph(paragraph) => {
                assert!(
                    !paragraph
                        .inlines
                        .iter()
                        .any(|inline| matches!(inline, Inline::Link(_)))
                );
                assert!(matches!(
                    paragraph.inlines.last(),
                    Some(Inline::Unknown(unknown))
                        if unknown.kind == "field:hyperlink"
                            && unknown.fallback_text.as_deref()
                                == Some("[field:hyperlink: not a url]")
                ));
            }
            other => panic!("expected paragraph block, got {other:?}"),
        }
        assert!(bridged.warnings.iter().any(|warning| {
            warning
                .message
                .contains("hyperlink field command was not URL-like")
        }));
    }

    #[test]
    fn preserves_non_url_hyperlink_control_as_unknown_inline() {
        let document = RhwpDocument {
            sections: vec![RhwpSection {
                paragraphs: vec![RhwpParagraph {
                    text: "body".to_string(),
                    controls: vec![Control::Hyperlink(RhwpHyperlink {
                        url: "not a url".to_string(),
                        text: "Example".to_string(),
                    })],
                    ..Default::default()
                }],
                ..Default::default()
            }],
            ..Default::default()
        };

        let bridged = BridgeContext::new(&document).into_document();

        match &bridged.sections[0].blocks[0] {
            Block::Paragraph(paragraph) => {
                assert!(
                    !paragraph
                        .inlines
                        .iter()
                        .any(|inline| matches!(inline, Inline::Link(_)))
                );
                assert!(matches!(
                    paragraph.inlines.last(),
                    Some(Inline::Unknown(unknown))
                        if unknown.kind == "hyperlink"
                            && unknown.fallback_text.as_deref()
                                == Some("[hyperlink: Example]")
                ));
            }
            other => panic!("expected paragraph block, got {other:?}"),
        }
        assert!(bridged.warnings.iter().any(|warning| {
            warning
                .message
                .contains("hyperlink control URL was not URL-like")
        }));
    }

    #[test]
    fn preserves_click_here_field_text_as_unknown_inline() {
        let document = RhwpDocument {
            sections: vec![RhwpSection {
                paragraphs: vec![RhwpParagraph {
                    text: "body".to_string(),
                    controls: vec![Control::Field(RhwpField {
                        field_type: RhwpFieldType::ClickHere,
                        command: RhwpField::build_clickhere_command(
                            "입력 안내",
                            "도움말",
                            "필드 이름",
                        ),
                        ..Default::default()
                    })],
                    ..Default::default()
                }],
                ..Default::default()
            }],
            ..Default::default()
        };

        let bridged = BridgeContext::new(&document).into_document();

        match &bridged.sections[0].blocks[0] {
            Block::Paragraph(paragraph) => match paragraph.inlines.last() {
                Some(Inline::Unknown(unknown)) => {
                    assert_eq!(unknown.kind, "field:clickhere");
                    assert_eq!(unknown.fallback_text.as_deref(), Some("입력 안내"));
                }
                other => panic!("expected click-here unknown inline, got {other:?}"),
            },
            other => panic!("expected paragraph block, got {other:?}"),
        }
        assert!(
            bridged
                .warnings
                .iter()
                .any(|warning| warning.message.contains("click-here fields"))
        );
    }

    #[test]
    fn preserves_visible_unsupported_controls_as_unknown_blocks() {
        let document = RhwpDocument {
            sections: vec![RhwpSection {
                paragraphs: vec![RhwpParagraph {
                    text: "body".to_string(),
                    controls: vec![
                        Control::Ruby(RhwpRuby {
                            ruby_text: "덧말".to_string(),
                            ..Default::default()
                        }),
                        Control::CharOverlap(RhwpCharOverlap {
                            chars: vec!['겹', '침'],
                            ..Default::default()
                        }),
                    ],
                    ..Default::default()
                }],
                ..Default::default()
            }],
            ..Default::default()
        };

        let bridged = BridgeContext::new(&document).into_document();
        let blocks = &bridged.sections[0].blocks;

        assert!(
            matches!(&blocks[1], Block::Unknown(unknown) if unknown.kind == "ruby" && unknown.fallback_text.as_deref() == Some("[ruby: 덧말]"))
        );
        assert!(
            matches!(&blocks[2], Block::Unknown(unknown) if unknown.kind == "char_overlap" && unknown.fallback_text.as_deref() == Some("[char overlap: 겹침]"))
        );
        assert!(bridged.warnings.iter().any(|warning| {
            warning
                .message
                .contains("unsupported visible control `ruby`")
        }));
    }

    #[test]
    fn preserves_hidden_comment_text_as_unknown_block() {
        let document = RhwpDocument {
            sections: vec![RhwpSection {
                paragraphs: vec![RhwpParagraph {
                    text: "body".to_string(),
                    controls: vec![Control::HiddenComment(Box::new(RhwpHiddenComment {
                        paragraphs: vec![RhwpParagraph {
                            text: "hidden note".to_string(),
                            ..Default::default()
                        }],
                    }))],
                    ..Default::default()
                }],
                ..Default::default()
            }],
            ..Default::default()
        };

        let bridged = BridgeContext::new(&document).into_document();
        let blocks = &bridged.sections[0].blocks;

        assert!(
            matches!(&blocks[1], Block::Unknown(unknown) if unknown.kind == "hidden_comment" && unknown.fallback_text.as_deref() == Some("[hidden comment]\nhidden note"))
        );
        assert!(
            bridged
                .warnings
                .iter()
                .any(|warning| { warning.message.contains("hidden comment paragraphs") })
        );
    }

    #[test]
    fn preserves_numbering_controls_as_unknown_blocks() {
        let document = RhwpDocument {
            sections: vec![RhwpSection {
                paragraphs: vec![RhwpParagraph {
                    text: "body".to_string(),
                    controls: vec![
                        Control::AutoNumber(RhwpAutoNumber {
                            number_type: RhwpAutoNumberType::Table,
                            number: 2,
                            assigned_number: 7,
                            ..Default::default()
                        }),
                        Control::NewNumber(RhwpNewNumber {
                            number_type: RhwpAutoNumberType::Picture,
                            number: 3,
                        }),
                        Control::PageNumberPos(RhwpPageNumberPos {
                            format: 4,
                            position: 5,
                            ..Default::default()
                        }),
                    ],
                    ..Default::default()
                }],
                ..Default::default()
            }],
            ..Default::default()
        };

        let bridged = BridgeContext::new(&document).into_document();
        let blocks = &bridged.sections[0].blocks;

        assert!(
            matches!(&blocks[1], Block::Unknown(unknown) if unknown.kind == "auto_number" && unknown.fallback_text.as_deref() == Some("[auto number: type=table, number=2, assigned=7]"))
        );
        assert!(
            matches!(&blocks[2], Block::Unknown(unknown) if unknown.kind == "new_number" && unknown.fallback_text.as_deref() == Some("[new number: type=picture, number=3]"))
        );
        assert!(
            matches!(&blocks[3], Block::Unknown(unknown) if unknown.kind == "page_number_position" && unknown.fallback_text.as_deref() == Some("[page number position: format=4, position=5]"))
        );
    }

    #[test]
    fn preserves_page_hide_and_form_controls_as_unknown_blocks() {
        let document = RhwpDocument {
            sections: vec![RhwpSection {
                paragraphs: vec![RhwpParagraph {
                    text: "body".to_string(),
                    controls: vec![
                        Control::PageHide(RhwpPageHide {
                            hide_header: true,
                            hide_page_num: true,
                            ..Default::default()
                        }),
                        Control::Form(Box::new(RhwpFormObject {
                            form_type: RhwpFormType::Edit,
                            name: "field1".to_string(),
                            text: "value".to_string(),
                            width: 100,
                            height: 200,
                            value: 1,
                            enabled: true,
                            ..Default::default()
                        })),
                    ],
                    ..Default::default()
                }],
                ..Default::default()
            }],
            ..Default::default()
        };

        let bridged = BridgeContext::new(&document).into_document();
        let blocks = &bridged.sections[0].blocks;

        assert!(
            matches!(&blocks[1], Block::Unknown(unknown) if unknown.kind == "page_hide" && unknown.fallback_text.as_deref() == Some("[page hide: header,page_num]"))
        );
        assert!(
            matches!(&blocks[2], Block::Unknown(unknown) if unknown.kind == "form" && unknown.fallback_text.as_deref() == Some("[form: type=edit, name=field1, text=value, value=1, enabled=true, size=100x200]"))
        );
    }

    #[test]
    fn warns_for_layout_controls_without_visible_fallback_blocks() {
        let document = RhwpDocument {
            sections: vec![RhwpSection {
                paragraphs: vec![RhwpParagraph {
                    text: "body".to_string(),
                    controls: vec![
                        Control::SectionDef(Box::new(RhwpSectionDef {
                            page_num: 3,
                            page_num_type: 1,
                            table_num: 4,
                            outline_numbering_id: 2,
                            hide_header: true,
                            page_def: rhwp::model::page::PageDef {
                                width: 59528,
                                height: 84188,
                                margin_left: 100,
                                margin_right: 200,
                                margin_top: 300,
                                margin_bottom: 400,
                                ..Default::default()
                            },
                            ..Default::default()
                        })),
                        Control::ColumnDef(RhwpColumnDef {
                            column_count: 1,
                            same_width: true,
                            ..Default::default()
                        }),
                        Control::ColumnDef(RhwpColumnDef {
                            column_type: RhwpColumnType::Parallel,
                            column_count: 2,
                            direction: RhwpColumnDirection::RightToLeft,
                            same_width: true,
                            spacing: 500,
                            widths: vec![1000, 2000],
                            gaps: vec![300],
                            separator_type: 1,
                            separator_width: 2,
                            separator_color: 0x00FF00,
                            ..Default::default()
                        }),
                    ],
                    ..Default::default()
                }],
                ..Default::default()
            }],
            ..Default::default()
        };

        let bridged = BridgeContext::new(&document).into_document();
        let blocks = &bridged.sections[0].blocks;

        assert_eq!(blocks.len(), 1);
        assert!(matches!(&blocks[0], Block::Paragraph(_)));
        assert!(bridged.warnings.iter().any(|warning| {
            warning.message.contains("layout control `section_def`")
                && warning.message.contains("page_num=3")
                && warning.message.contains("hidden=header")
        }));
        assert!(bridged.warnings.iter().any(|warning| {
            warning.message.contains("layout control `column_def`")
                && warning.message.contains("column_count=2")
                && warning.message.contains("direction=right_to_left")
        }));
        assert!(
            !bridged
                .warnings
                .iter()
                .any(|warning| warning.message.contains("column_count=1"))
        );
    }

    #[test]
    fn warns_when_known_controls_are_not_semantically_mapped() {
        let document = RhwpDocument {
            sections: vec![RhwpSection {
                paragraphs: vec![RhwpParagraph {
                    text: "body".to_string(),
                    controls: vec![
                        Control::Bookmark(RhwpBookmark {
                            name: "target".to_string(),
                        }),
                        Control::Field(RhwpField {
                            field_type: RhwpFieldType::Date,
                            command: "date".to_string(),
                            ..Default::default()
                        }),
                    ],
                    ..Default::default()
                }],
                ..Default::default()
            }],
            ..Default::default()
        };

        let bridged = BridgeContext::new(&document).into_document();

        assert!(
            bridged
                .warnings
                .iter()
                .any(|warning| warning.message.contains("bookmark controls"))
        );
        assert!(bridged.warnings.iter().any(|warning| {
            warning.code == WarningCode::Unknown && warning.message.contains("`field:date`")
        }));
        match &bridged.sections[0].blocks[0] {
            Block::Paragraph(paragraph) => {
                assert!(paragraph.inlines.iter().any(|inline| {
                    matches!(
                        inline,
                        Inline::Anchor { id } if id == "target"
                    )
                }));

                match paragraph.inlines.last() {
                    Some(Inline::Unknown(unknown)) => {
                        assert_eq!(unknown.kind, "field:date");
                        assert_eq!(unknown.fallback_text.as_deref(), Some("[field:date: date]"));
                    }
                    other => panic!("expected date field unknown inline, got {other:?}"),
                }
            }
            other => panic!("expected paragraph block, got {other:?}"),
        }
    }

    #[test]
    fn preserves_text_inside_non_link_field_ranges() {
        let document = RhwpDocument {
            sections: vec![RhwpSection {
                paragraphs: vec![RhwpParagraph {
                    text: "date text".to_string(),
                    field_ranges: vec![FieldRange {
                        control_idx: 0,
                        start_char_idx: 0,
                        end_char_idx: 4,
                    }],
                    controls: vec![Control::Field(RhwpField {
                        field_type: RhwpFieldType::Date,
                        command: "date".to_string(),
                        ..Default::default()
                    })],
                    ..Default::default()
                }],
                ..Default::default()
            }],
            ..Default::default()
        };

        let bridged = BridgeContext::new(&document).into_document();

        match &bridged.sections[0].blocks[0] {
            Block::Paragraph(paragraph) => {
                let text = crate::util::plain_text::blocks_to_plain_text(&[Block::Paragraph(
                    paragraph.clone(),
                )]);
                assert!(text.contains("date text"));
                assert!(paragraph.inlines.iter().any(|inline| {
                    matches!(
                        inline,
                        Inline::Unknown(unknown)
                            if unknown.kind == "field:date"
                                && unknown.fallback_text.as_deref() == Some("[field:date: date]")
                    )
                }));
            }
            other => panic!("expected paragraph block, got {other:?}"),
        }
    }

    #[test]
    fn preserves_unknown_controls_as_unknown_blocks_and_warnings() {
        let document = RhwpDocument {
            sections: vec![RhwpSection {
                paragraphs: vec![RhwpParagraph {
                    controls: vec![Control::Unknown(RhwpUnknownControl {
                        ctrl_id: 0x1234ABCD,
                    })],
                    ..Default::default()
                }],
                ..Default::default()
            }],
            ..Default::default()
        };

        let bridged = BridgeContext::new(&document).into_document();

        assert!(matches!(
            &bridged.sections[0].blocks[0],
            Block::Unknown(unknown) if unknown.kind == "control:0x1234abcd"
        ));
        assert!(bridged.warnings.iter().any(|warning| {
            warning
                .message
                .contains("unknown control `control:0x1234abcd`")
        }));
    }

    #[test]
    fn maps_footnotes_into_note_store_and_inline_refs() {
        let document = RhwpDocument {
            sections: vec![RhwpSection {
                paragraphs: vec![RhwpParagraph {
                    text: "body".to_string(),
                    controls: vec![Control::Footnote(Box::new(RhwpFootnote {
                        number: 3,
                        paragraphs: vec![RhwpParagraph {
                            text: "note body".to_string(),
                            ..Default::default()
                        }],
                    }))],
                    ..Default::default()
                }],
                ..Default::default()
            }],
            ..Default::default()
        };

        let bridged = BridgeContext::new(&document).into_document();

        assert_eq!(bridged.notes.notes.len(), 1);
        assert_eq!(bridged.notes.notes[0].id.as_str(), "footnote-3");
        assert_eq!(bridged.notes.notes[0].kind, NoteKind::Footnote);
        assert_eq!(bridged.warnings.len(), 1);

        match &bridged.sections[0].blocks[0] {
            Block::Paragraph(paragraph) => match paragraph.inlines.last() {
                Some(Inline::FootnoteRef { note_id }) => {
                    assert_eq!(note_id.as_str(), "footnote-3");
                }
                other => panic!("expected trailing footnote ref, got {other:?}"),
            },
            other => panic!("expected paragraph block, got {other:?}"),
        }
    }

    #[test]
    fn maps_headers_and_footers_into_section_metadata() {
        let document = RhwpDocument {
            sections: vec![RhwpSection {
                paragraphs: vec![RhwpParagraph {
                    controls: vec![
                        Control::Header(Box::new(RhwpHeader {
                            apply_to: RhwpHeaderFooterApply::Both,
                            paragraphs: vec![RhwpParagraph {
                                text: "header".to_string(),
                                ..Default::default()
                            }],
                            ..Default::default()
                        })),
                        Control::Footer(Box::new(RhwpFooter {
                            apply_to: RhwpHeaderFooterApply::Even,
                            paragraphs: vec![RhwpParagraph {
                                text: "footer".to_string(),
                                ..Default::default()
                            }],
                            ..Default::default()
                        })),
                    ],
                    ..Default::default()
                }],
                ..Default::default()
            }],
            ..Default::default()
        };

        let bridged = BridgeContext::new(&document).into_document();

        assert_eq!(bridged.sections[0].headers.len(), 1);
        assert_eq!(bridged.sections[0].footers.len(), 1);
        assert_eq!(
            bridged.sections[0].headers[0].placement,
            HeaderFooterPlacement::Default
        );
        assert_eq!(
            bridged.sections[0].footers[0].placement,
            HeaderFooterPlacement::EvenPage
        );
    }

    #[test]
    fn maps_bullet_list_info_from_para_shape() {
        let document = RhwpDocument {
            doc_info: DocInfo {
                para_shapes: vec![RhwpParaShape {
                    head_type: RhwpHeadType::Bullet,
                    para_level: 1,
                    numbering_id: 1,
                    ..Default::default()
                }],
                bullets: vec![RhwpBullet {
                    bullet_char: '•',
                    ..Default::default()
                }],
                ..Default::default()
            },
            sections: vec![RhwpSection {
                paragraphs: vec![RhwpParagraph {
                    text: "item".to_string(),
                    para_shape_id: 0,
                    ..Default::default()
                }],
                ..Default::default()
            }],
            ..Default::default()
        };

        let bridged = BridgeContext::new(&document).into_document();

        match &bridged.sections[0].blocks[0] {
            Block::Paragraph(paragraph) => {
                let list = paragraph.list.as_ref().expect("list info should exist");
                assert_eq!(list.kind, ListKind::Unordered);
                assert_eq!(list.level, 1);
                assert_eq!(list.marker.as_deref(), Some("•"));
            }
            other => panic!("expected paragraph block, got {other:?}"),
        }
    }

    #[test]
    fn maps_ordered_list_numbers_from_numbering_state() {
        let document = RhwpDocument {
            doc_info: DocInfo {
                para_shapes: vec![RhwpParaShape {
                    head_type: RhwpHeadType::Number,
                    para_level: 0,
                    numbering_id: 1,
                    ..Default::default()
                }],
                numberings: vec![RhwpNumbering {
                    level_start_numbers: [1, 1, 1, 1, 1, 1, 1],
                    ..Default::default()
                }],
                ..Default::default()
            },
            sections: vec![RhwpSection {
                paragraphs: vec![
                    RhwpParagraph {
                        text: "first".to_string(),
                        para_shape_id: 0,
                        ..Default::default()
                    },
                    RhwpParagraph {
                        text: "second".to_string(),
                        para_shape_id: 0,
                        numbering_restart: Some(RhwpNumberingRestart::ContinuePrevious),
                        ..Default::default()
                    },
                ],
                ..Default::default()
            }],
            ..Default::default()
        };

        let bridged = BridgeContext::new(&document).into_document();

        match &bridged.sections[0].blocks[0] {
            Block::Paragraph(paragraph) => {
                assert_eq!(
                    paragraph.list.as_ref().and_then(|list| list.number),
                    Some(1)
                );
            }
            other => panic!("expected paragraph block, got {other:?}"),
        }

        match &bridged.sections[0].blocks[1] {
            Block::Paragraph(paragraph) => {
                assert_eq!(
                    paragraph.list.as_ref().and_then(|list| list.number),
                    Some(2)
                );
            }
            other => panic!("expected paragraph block, got {other:?}"),
        }
    }

    #[test]
    fn maps_outline_paragraph_role_from_para_shape() {
        let document = RhwpDocument {
            doc_info: DocInfo {
                para_shapes: vec![RhwpParaShape {
                    head_type: RhwpHeadType::Outline,
                    para_level: 2,
                    numbering_id: 1,
                    ..Default::default()
                }],
                numberings: vec![RhwpNumbering {
                    level_start_numbers: [1, 1, 1, 1, 1, 1, 1],
                    ..Default::default()
                }],
                ..Default::default()
            },
            sections: vec![RhwpSection {
                paragraphs: vec![RhwpParagraph {
                    text: "outline".to_string(),
                    para_shape_id: 0,
                    ..Default::default()
                }],
                section_def: RhwpSectionDef {
                    outline_numbering_id: 1,
                    ..Default::default()
                },
                ..Default::default()
            }],
            ..Default::default()
        };

        let bridged = BridgeContext::new(&document).into_document();

        match &bridged.sections[0].blocks[0] {
            Block::Paragraph(paragraph) => {
                assert_eq!(paragraph.role, ParagraphRole::Heading { level: 3 });
                assert!(matches!(
                    paragraph.list.as_ref().map(|list| &list.kind),
                    Some(ListKind::Ordered)
                ));
            }
            other => panic!("expected paragraph block, got {other:?}"),
        }
    }

    #[test]
    fn preview_fallback_marks_warning() {
        let document = document_from_hwpx_fallback(
            Document::from_paragraphs(vec!["preview".to_string()]),
            HwpxTextFallbackSource::PreviewText,
        );

        assert_eq!(document.sections.len(), 1);
        assert_eq!(document.warnings.len(), 1);
        assert_eq!(
            document.warnings[0].code,
            WarningCode::UsedHwpxPreviewFallback
        );
    }
}
