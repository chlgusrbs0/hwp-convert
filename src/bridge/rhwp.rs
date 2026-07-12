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
    Alignment as RhwpAlignment, BorderFill as RhwpBorderFill, BorderLine as RhwpBorderLine,
    BorderLineType as RhwpBorderLineType, CharShape as RhwpCharShape, FillType as RhwpFillType,
    HeadType as RhwpHeadType, Numbering as RhwpNumbering, ParaShape as RhwpParaShape,
    ShapeBorderLine as RhwpShapeBorderLine, UnderlineType as RhwpUnderlineType,
};
use rhwp::model::table::{
    Cell as RhwpCell, Table as RhwpTable, VerticalAlign as RhwpVerticalAlign,
};
use rhwp::renderer::{NumberFormat as RhwpNumberFormat, format_number as format_rhwp_number};

use crate::hwpx::{self, HwpxTextFallbackSource, InputKind};
use crate::ir::{
    Block, Border, BorderStyle, CaptionPlacement, Color, ConversionWarning, Document, Equation,
    EquationKind, HeaderFooter, HeaderFooterPlacement, HorizontalObjectAlignment,
    HorizontalRelativeTo, Image, ImageCrop, ImageEffect as IrImageEffect, ImagePlacement,
    ImageResource, ImageTextWrap, Inline, LengthPt, LengthPx, Link, ListInfo, ListKind,
    NamedParagraphStyle, NamedTextStyle, Note, NoteId, NoteKind, NoteStore, Paragraph,
    ParagraphRole, ParagraphStyle, ParagraphStyleId, Percent, Resource, ResourceId, ResourceStore,
    Section, Shape, ShapeKind, Spacing, StyleSheet, Table, TableCell, TableCellStyle,
    TablePageBreak, TableRow, TableStyle, TextDecorationStyle, TextRun, TextStyle, TextStyleId,
    UnknownInline, VerticalAlign, VerticalObjectAlignment, VerticalRelativeTo, WarningCode,
};

use super::hwpx_reconcile;

/// Map rhwp cell vertical alignment to the IR. `Top` is rhwp's default, so it is
/// represented as `None` to keep the IR and JSON output free of redundant data.
fn map_vertical_align(value: RhwpVerticalAlign) -> Option<VerticalAlign> {
    match value {
        RhwpVerticalAlign::Top => None,
        RhwpVerticalAlign::Center => Some(VerticalAlign::Middle),
        RhwpVerticalAlign::Bottom => Some(VerticalAlign::Bottom),
    }
}

/// Parse a source document with `rhwp` and bridge the resulting model into the
/// local `Document` IR. For `.hwpx`, section XML fallback remains available
/// when parsing fails or when the mapped body is structurally empty. A
/// successful HWPX parse is also compared with the section XML fallback so
/// partial rHWP data loss is either recovered conservatively or reported.
pub fn read_document(input_path: &Path) -> Result<Document, Box<dyn Error>> {
    let (input_kind, bytes) = hwpx::read_input_bytes(input_path)?;

    match rhwp::parse_document(&bytes) {
        Ok(parsed) => {
            let bridged = BridgeContext::new(&parsed).into_document();
            if document_has_blocks(&bridged) {
                if input_kind == InputKind::Hwpx {
                    Ok(reconcile_partial_hwpx_document(&bytes, bridged))
                } else {
                    Ok(bridged)
                }
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

fn reconcile_partial_hwpx_document(bytes: &[u8], bridged: Document) -> Document {
    match hwpx::read_section_document_from_archive(bytes) {
        Ok(fallback) => hwpx_reconcile::reconcile(bridged, fallback),
        Err(_) => bridged,
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
        let mapped_paragraph = self.map_paragraph(paragraph, outline_numbering_id, list_state);
        let control_positions = infer_control_text_positions(paragraph);
        let mapped_controls = paragraph
            .controls
            .iter()
            .enumerate()
            .map(|(index, control)| {
                (
                    control_positions.get(index).copied().flatten(),
                    self.map_control_blocks(control),
                )
            })
            .collect::<Vec<_>>();
        let text_len = paragraph.text.chars().count();
        let can_place_around_paragraph = text_len > 0
            && mapped_controls.iter().all(|(position, mapped)| {
                mapped.is_empty()
                    || position.is_some_and(|position| position == 0 || position >= text_len)
            });

        if can_place_around_paragraph {
            for (position, mapped) in &mapped_controls {
                if *position == Some(0) {
                    blocks.extend(mapped.iter().cloned());
                }
            }
            if let Some(mapped) = mapped_paragraph {
                blocks.push(Block::Paragraph(mapped));
            }
            for (position, mapped) in mapped_controls {
                if position.is_some_and(|position| position >= text_len) {
                    blocks.extend(mapped);
                }
            }
            return;
        }

        let has_unplaced_visible_control = text_len > 0
            && mapped_controls
                .iter()
                .any(|(position, mapped)| !mapped.is_empty() && position != &Some(text_len));
        if has_unplaced_visible_control {
            self.add_warning_once(
                "Some rhwp block controls occur inside paragraph text or lack recoverable offsets; Document IR kept the paragraph intact and placed those controls after it, so exact reading order may differ.",
            );
        }

        if let Some(mapped) = mapped_paragraph {
            blocks.push(Block::Paragraph(mapped));
        }
        for (_, mapped) in mapped_controls {
            blocks.extend(mapped);
        }
    }

    fn map_paragraph(
        &mut self,
        paragraph: &RhwpParagraph,
        outline_numbering_id: u16,
        list_state: &mut ListState,
    ) -> Option<Paragraph> {
        let inlines = self.map_paragraph_inlines(paragraph);
        if inlines.is_empty() && !paragraph.controls.is_empty() {
            return None;
        }

        Some(Paragraph {
            role: self.map_paragraph_role(paragraph),
            inlines,
            style: self.map_paragraph_style_by_id(paragraph.para_shape_id, "paragraph style"),
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

    fn find_unique_substring_char_index(&self, text: &str, substring: &str) -> Option<usize> {
        let mut matches = text.match_indices(substring);
        let (byte_idx, _) = matches.next()?;
        if matches.next().is_some() {
            return None;
        }
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
        let control_positions = infer_control_text_positions(paragraph);

        let mut insertions = Vec::new();
        let mut imprecise_note_placement = false;
        let mut imprecise_link_placement = false;
        let mut imprecise_field_placement = false;

        for (index, control) in paragraph.controls.iter().enumerate() {
            if consumed_control_idxs.contains(&index) {
                continue;
            }

            match control {
                Control::Footnote(note) => {
                    let note_id =
                        self.store_note(NoteKind::Footnote, note.number, &note.paragraphs);
                    let inline = Inline::FootnoteRef { note_id };
                    let preferred = control_positions.get(index).copied().flatten();
                    imprecise_note_placement |= preferred.is_none();
                    insertions.push(InlineInsertion {
                        position: preferred,
                        control_idx: index,
                        inline,
                    });
                }
                Control::Endnote(note) => {
                    let note_id = self.store_note(NoteKind::Endnote, note.number, &note.paragraphs);
                    let inline = Inline::EndnoteRef { note_id };
                    let preferred = control_positions.get(index).copied().flatten();
                    imprecise_note_placement |= preferred.is_none();
                    insertions.push(InlineInsertion {
                        position: preferred,
                        control_idx: index,
                        inline,
                    });
                }
                Control::Hyperlink(link) => {
                    if let Some(mapped) = self.map_trailing_hyperlink(link) {
                        let exact = control_positions.get(index).copied().flatten();
                        let preferred = exact.or_else(|| {
                            non_empty_string(&link.text).and_then(|text| {
                                self.find_unique_substring_char_index(&paragraph.text, &text)
                            })
                        });
                        imprecise_link_placement |= exact.is_none();
                        insertions.push(InlineInsertion {
                            position: preferred,
                            control_idx: index,
                            inline: Inline::Link(mapped),
                        });
                    } else if let Some(inline) = self.map_hyperlink_fallback(link) {
                        self.add_warning_once(
                            "rhwp hyperlink control URL was not URL-like; hwp-convert preserved it as unknown inline fallback text.",
                        );
                        let preferred = control_positions.get(index).copied().flatten();
                        imprecise_field_placement |= preferred.is_none();
                        insertions.push(InlineInsertion {
                            position: preferred,
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
                        let exact = control_positions.get(index).copied().flatten();
                        let preferred = exact.or_else(|| {
                            (!label.is_empty())
                                .then_some(label.as_str())
                                .and_then(|label| {
                                    self.find_unique_substring_char_index(&paragraph.text, label)
                                })
                        });
                        imprecise_link_placement |= exact.is_none();
                        insertions.push(InlineInsertion {
                            position: preferred,
                            control_idx: index,
                            inline: Inline::Link(mapped),
                        });
                    } else if let Some(inline) = self.map_field_fallback(field) {
                        self.add_warning_once(
                            "rhwp hyperlink field command was not URL-like; hwp-convert preserved it as unknown inline fallback text.",
                        );
                        let preferred = control_positions.get(index).copied().flatten();
                        imprecise_field_placement |= preferred.is_none();
                        insertions.push(InlineInsertion {
                            position: preferred,
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
                            let exact = control_positions.get(index).copied().flatten();
                            let preferred = exact.or_else(|| {
                                (!fallback_text.is_empty())
                                    .then_some(fallback_text.as_str())
                                    .and_then(|text| {
                                        self.find_unique_substring_char_index(&paragraph.text, text)
                                    })
                            });
                            imprecise_field_placement |= exact.is_none();
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
                        let exact = control_positions.get(index).copied().flatten();
                        let preferred = exact.or_else(|| {
                            (!fallback_text.is_empty())
                                .then_some(fallback_text.as_str())
                                .and_then(|text| {
                                    self.find_unique_substring_char_index(&paragraph.text, text)
                                })
                        });
                        imprecise_field_placement |= exact.is_none();
                        insertions.push(InlineInsertion {
                            position: preferred,
                            control_idx: index,
                            inline,
                        });
                    }
                }
                Control::Bookmark(bookmark) => {
                    if let Some(mapped) = self.map_bookmark_anchor(bookmark) {
                        let preferred =
                            control_positions.get(index).copied().flatten().or_else(|| {
                                non_empty_string(&bookmark.name).and_then(|name| {
                                    self.find_unique_substring_char_index(&paragraph.text, &name)
                                })
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

        if imprecise_note_placement {
            self.add_warning_once(
                "Some rhwp footnote/endnote positions could not be recovered from paragraph offsets, so note references were appended after paragraph text.",
            );
        }

        if imprecise_link_placement {
            self.add_warning_once(
                "Some rhwp hyperlinks could not be placed from paragraph offsets, so bridge fallback used a unique matching label or appended them after paragraph text.",
            );
        }

        if imprecise_field_placement {
            self.add_warning_once(
                "Some rhwp field controls could not be placed from paragraph offsets, so bridge fallback used uniquely matching text or appended their fallback text after paragraph text.",
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
        &mut self,
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

    fn build_text_segments(
        &mut self,
        paragraph: &RhwpParagraph,
        text_len: usize,
    ) -> Vec<TextSegment> {
        if text_len == 0 {
            return Vec::new();
        }

        let fallback_style_id = match self.source.doc_info.styles.get(paragraph.style_id as usize) {
            Some(style) => Some(style.char_shape_id as u32),
            None => {
                if !self.source.doc_info.styles.is_empty() {
                    self.add_warning_once(&format!(
                        "rhwp paragraph referenced missing style id {}; hwp-convert used direct char shape refs or default text style.",
                        paragraph.style_id
                    ));
                }
                None
            }
        };
        let fallback_style = fallback_style_id
            .and_then(|char_shape_id| {
                self.map_text_style_for_language_by_char_shape_id_or_warn(
                    char_shape_id,
                    0,
                    "paragraph named style",
                )
            })
            .unwrap_or_default();
        let fallback_style_ref = self.text_style_ref(paragraph.style_id);

        let mut refs = paragraph.char_shapes.clone();
        refs.sort_by_key(|char_shape| char_shape.start_pos);

        if refs.is_empty() {
            return vec![TextSegment {
                start: 0,
                end: text_len,
                char_shape_id: fallback_style_id,
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
            let mapped_style = self.map_text_style_for_language_by_char_shape_id_or_warn(
                char_shape_ref.char_shape_id,
                0,
                "paragraph text run",
            );
            let char_shape_id = mapped_style
                .as_ref()
                .map(|_| char_shape_ref.char_shape_id)
                .or(fallback_style_id);
            let style = mapped_style.unwrap_or_else(|| fallback_style.clone());
            let style_ref = if fallback_style_id == Some(char_shape_ref.char_shape_id) {
                fallback_style_ref.clone()
            } else {
                None
            };

            segments.push(TextSegment {
                start,
                end,
                char_shape_id,
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
        &mut self,
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
            for (language_index, fragment) in split_text_by_language(&text) {
                let style = segment
                    .char_shape_id
                    .and_then(|char_shape_id| {
                        self.map_text_style_for_language_by_char_shape_id_or_warn(
                            char_shape_id,
                            language_index,
                            "paragraph text run",
                        )
                    })
                    .unwrap_or_else(|| segment.style.clone());
                push_text_fragment(inlines, &fragment, &style, segment.style_ref.as_ref());
            }
        }
    }

    fn map_link_from_field_range(
        &mut self,
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
        let fallback_text =
            first_non_empty_string([non_empty_string(&link.text), non_empty_string(&link.url)])?;

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
        &mut self,
        paragraph: &RhwpParagraph,
        outline_numbering_id: u16,
        list_state: &mut ListState,
    ) -> Option<ListInfo> {
        let para_shape = self.lookup_para_shape(paragraph.para_shape_id).cloned()?;
        let level = para_shape.para_level.min(6);

        match para_shape.head_type {
            RhwpHeadType::None => None,
            RhwpHeadType::Bullet => {
                let bullet = para_shape
                    .numbering_id
                    .checked_sub(1)
                    .and_then(|index| self.source.doc_info.bullets.get(index as usize))
                    .cloned();
                let marker = bullet
                    .as_ref()
                    .and_then(|bullet| normalize_bullet_char(bullet.bullet_char))
                    .map(|ch| ch.to_string());
                if para_shape.numbering_id != 0 && bullet.is_none() {
                    self.add_warning_once(&format!(
                        "rhwp bullet paragraph referenced missing bullet id {}; hwp-convert used default unordered list marker behavior.",
                        para_shape.numbering_id
                    ));
                }
                if let Some(bullet) = bullet.as_ref() {
                    if marker.is_none() {
                        self.add_warning_once(&format!(
                            "rhwp bullet id {} exposed unusable marker {:?}; hwp-convert used default unordered list marker behavior.",
                            para_shape.numbering_id, bullet.bullet_char
                        ));
                    }
                    self.warn_unmodeled_bullet(bullet, para_shape.numbering_id);
                }
                Some(ListInfo {
                    kind: ListKind::Unordered,
                    level,
                    marker,
                    marker_format: None,
                    number: None,
                })
            }
            RhwpHeadType::Number | RhwpHeadType::Outline => {
                let numbering_id = resolve_numbering_id(&para_shape, outline_numbering_id);
                let numbering_index = numbering_id.checked_sub(1);
                let numbering = numbering_index
                    .and_then(|index| self.source.doc_info.numberings.get(index as usize))
                    .cloned();
                if numbering_id != 0 && numbering.is_none() {
                    self.add_warning_once(&format!(
                        "rhwp ordered paragraph referenced missing numbering id {numbering_id}; hwp-convert used sequential fallback numbering."
                    ));
                }
                let marker_format = numbering
                    .as_ref()
                    .and_then(|numbering| numbering.level_formats.get(level as usize))
                    .filter(|format| !format.is_empty())
                    .cloned();
                if let Some(numbering) = numbering.as_ref() {
                    self.warn_unmodeled_numbering(
                        numbering,
                        numbering_id,
                        level,
                        marker_format.as_deref(),
                    );
                }
                let number = numbering_id.checked_sub(1).and_then(|_| {
                    list_state.advance(
                        numbering_id,
                        level,
                        paragraph.numbering_restart,
                        numbering.as_ref(),
                    )
                });
                let marker = marker_format.as_deref().and_then(|format| {
                    numbering.as_ref().and_then(|numbering| {
                        list_state
                            .counters(numbering_id)
                            .map(|counters| expand_ordered_marker(format, counters, numbering))
                    })
                });

                Some(ListInfo {
                    kind: ListKind::Ordered,
                    level,
                    marker,
                    marker_format,
                    number,
                })
            }
        }
    }

    fn warn_unmodeled_bullet(&mut self, bullet: &rhwp::model::style::Bullet, bullet_id: u16) {
        if bullet.image_bullet != 0 {
            self.add_warning_once(&format!(
                "rhwp bullet id {bullet_id} used image bullet {}; ListInfo preserved only a text marker and omitted the image bullet resource.",
                bullet.image_bullet
            ));
        }
        if bullet.width_adjust != 0
            || bullet.text_distance != 0
            || bullet.attr != 0
            || bullet.image_data != [0; 4]
        {
            self.add_warning_once(&format!(
                "rhwp bullet id {bullet_id} used layout/style metadata attr={}, width_adjust={}, text_distance={}, image_data={:?}; ListInfo omitted those properties.",
                bullet.attr, bullet.width_adjust, bullet.text_distance, bullet.image_data
            ));
        }
        if normalize_bullet_char(bullet.check_bullet_char).is_some() {
            self.add_warning_once(&format!(
                "rhwp bullet id {bullet_id} exposed a check-bullet marker {:?}; ListInfo preserved only the primary bullet marker.",
                bullet.check_bullet_char
            ));
        }
    }

    fn warn_unmodeled_numbering(
        &mut self,
        numbering: &RhwpNumbering,
        numbering_id: u16,
        level: u8,
        marker: Option<&str>,
    ) {
        let head = &numbering.heads[level as usize];
        let char_shape_id =
            (!matches!(head.char_shape_id, 0 | u32::MAX)).then_some(head.char_shape_id);
        if let Some(marker) = marker {
            for referenced_level in numbering_marker_level_references(marker) {
                let format_code = numbering.heads[referenced_level].number_format;
                if numbering_format(format_code).is_none() {
                    self.add_warning_once(&format!(
                        "rhwp numbering id {numbering_id} level {level} format {marker:?} referenced unsupported number format code {format_code} at level {}; hwp-convert approximated that marker component with decimal digits.",
                        referenced_level + 1
                    ));
                }
            }
        }
        if head.attr != 0
            || head.width_adjust != 0
            || head.text_distance != 0
            || char_shape_id.is_some()
        {
            let char_shape_id = char_shape_id
                .map(|id| id.to_string())
                .unwrap_or_else(|| "-".to_string());
            self.add_warning_once(&format!(
                "rhwp numbering id {numbering_id} level {level} used layout/style metadata attr={}, width_adjust={}, text_distance={}, char_shape_id={}; ListInfo omitted those properties.",
                head.attr,
                head.width_adjust,
                head.text_distance,
                char_shape_id
            ));
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
        self.warn_unmodeled_table_properties(table);
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
                    .map(|cell| self.map_table_cell(cell, &table.padding))
                    .collect(),
                height: table
                    .row_sizes
                    .get(row_index as usize)
                    .copied()
                    .and_then(i16_hwp_units_to_px_option),
            });
        }

        Table {
            rows,
            style: TableStyle {
                background_color: self
                    .border_fill_background_color(table.border_fill_id, "table background"),
                width: hwp_units_to_px_option(table.common.width),
                height: hwp_units_to_px_option(table.common.height),
                margin_top: i16_hwp_units_to_px_option(table.outer_margin_top),
                margin_right: i16_hwp_units_to_px_option(table.outer_margin_right),
                margin_bottom: i16_hwp_units_to_px_option(table.outer_margin_bottom),
                margin_left: i16_hwp_units_to_px_option(table.outer_margin_left),
                cell_spacing: i16_hwp_units_to_px_option(table.cell_spacing),
                repeat_header: table.repeat_header,
                page_break: match table.page_break {
                    rhwp::model::table::TablePageBreak::None => None,
                    rhwp::model::table::TablePageBreak::CellBreak => Some(TablePageBreak::Cell),
                    rhwp::model::table::TablePageBreak::RowBreak => Some(TablePageBreak::Row),
                },
            },
        }
    }

    fn warn_unmodeled_table_properties(&mut self, table: &RhwpTable) {
        let mut details = Vec::new();

        if table.cell_spacing < 0 {
            details.push(format!("negative_cell_spacing={}", table.cell_spacing));
        }
        if !table.zones.is_empty() {
            details.push(format!("border_fill_zones={}", table.zones.len()));
        }
        if table.outer_margin_left < 0
            || table.outer_margin_right < 0
            || table.outer_margin_top < 0
            || table.outer_margin_bottom < 0
        {
            details.push(format!(
                "negative_outer_margins={}/{}/{}/{}",
                table.outer_margin_left,
                table.outer_margin_right,
                table.outer_margin_top,
                table.outer_margin_bottom
            ));
        }

        let common = &table.common;
        if common.horizontal_offset != 0
            || common.vertical_offset != 0
            || common.z_order != 0
            || common.margin.left != 0
            || common.margin.right != 0
            || common.margin.top != 0
            || common.margin.bottom != 0
            || common.treat_as_char
            || common.text_wrap != rhwp::model::shape::TextWrap::Square
            || common.vert_rel_to != rhwp::model::shape::VertRelTo::Paper
            || common.horz_rel_to != rhwp::model::shape::HorzRelTo::Paper
            || common.vert_align != rhwp::model::shape::VertAlign::Top
            || common.horz_align != rhwp::model::shape::HorzAlign::Left
        {
            details.push(format!(
                "layout=offset:{}/{},z:{},treat_as_char:{},wrap:{:?}",
                common.horizontal_offset,
                common.vertical_offset,
                common.z_order,
                common.treat_as_char,
                common.text_wrap
            ));
        }

        if details.is_empty() {
            return;
        }

        self.add_warning_once(&format!(
            "rhwp exposed table layout properties that Table IR does not model; hwp-convert preserved table structure but omitted {}.",
            details.join(", ")
        ));
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

    fn map_table_cell(
        &mut self,
        cell: &RhwpCell,
        table_padding: &rhwp::model::Padding,
    ) -> TableCell {
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

        let [border_left, border_right, border_top, border_bottom] =
            self.map_borders(cell.border_fill_id, "table cell borders");
        let padding = if cell.apply_inner_margin {
            &cell.padding
        } else {
            table_padding
        };
        self.warn_negative_table_padding(
            padding,
            if cell.apply_inner_margin {
                "cell-specific"
            } else {
                "table-default"
            },
        );

        TableCell {
            row_span: (cell.row_span as u32).max(1),
            col_span: (cell.col_span as u32).max(1),
            is_header: cell.is_header,
            blocks,
            style: TableCellStyle {
                background_color: self
                    .border_fill_background_color(cell.border_fill_id, "table cell background"),
                vertical_align: map_vertical_align(cell.vertical_align),
                width: hwp_units_to_px_option(cell.width),
                height: hwp_units_to_px_option(cell.height),
                padding_top: i16_hwp_units_to_px_option(padding.top),
                padding_right: i16_hwp_units_to_px_option(padding.right),
                padding_bottom: i16_hwp_units_to_px_option(padding.bottom),
                padding_left: i16_hwp_units_to_px_option(padding.left),
                border_top,
                border_right,
                border_bottom,
                border_left,
            },
        }
    }

    fn map_picture_block(&mut self, picture: &Picture) -> Block {
        self.warn_unsupported_picture_transform(picture);
        let caption_placement = picture
            .caption
            .as_ref()
            .map(|caption| match caption.direction {
                RhwpCaptionDirection::Left => CaptionPlacement::Left,
                RhwpCaptionDirection::Right => CaptionPlacement::Right,
                RhwpCaptionDirection::Top => CaptionPlacement::Top,
                RhwpCaptionDirection::Bottom => CaptionPlacement::Bottom,
            });

        match self.ensure_image_resource(picture.image_attr.bin_data_id) {
            Some(resource_id) => Block::Image(Image {
                resource_id,
                alt: non_empty_string(&picture.common.description),
                caption: self.caption_plain_text(
                    picture.caption.as_ref().map(|caption| &caption.paragraphs),
                ),
                caption_placement,
                crop: map_picture_crop(picture),
                width: hwp_units_to_px_option(picture.common.width),
                height: hwp_units_to_px_option(picture.common.height),
                border: map_image_border(picture),
                grayscale: matches!(
                    picture.image_attr.effect,
                    RhwpImageEffect::GrayScale | RhwpImageEffect::BlackWhite
                ),
                effect: match picture.image_attr.effect {
                    RhwpImageEffect::RealPic => None,
                    RhwpImageEffect::GrayScale => Some(IrImageEffect::Grayscale),
                    RhwpImageEffect::BlackWhite => Some(IrImageEffect::BlackWhite),
                    RhwpImageEffect::Pattern8x8 => Some(IrImageEffect::Pattern8x8),
                },
                placement: map_picture_placement(picture),
                brightness: (picture.image_attr.brightness != 0)
                    .then_some(i32::from(picture.image_attr.brightness)),
                contrast: (picture.image_attr.contrast != 0)
                    .then_some(i32::from(picture.image_attr.contrast)),
                opacity: None,
                rotation_degrees: (picture.shape_attr.rotation_angle != 0)
                    .then_some(picture.shape_attr.rotation_angle as f32),
                flip_horizontal: picture.shape_attr.horz_flip.then_some(true),
                flip_vertical: picture.shape_attr.vert_flip.then_some(true),
                padding_top: i16_hwp_units_to_px_option(picture.padding.top),
                padding_right: i16_hwp_units_to_px_option(picture.padding.right),
                padding_bottom: i16_hwp_units_to_px_option(picture.padding.bottom),
                padding_left: i16_hwp_units_to_px_option(picture.padding.left),
            }),
            None => {
                self.add_warning_once(&format!(
                    "rhwp picture referenced missing bin data {}; hwp-convert preserved an image placeholder.",
                    picture.image_attr.bin_data_id
                ));
                Block::Unknown(crate::ir::UnknownBlock {
                    kind: "picture".to_string(),
                    fallback_text: Some("[image]".to_string()),
                    message: Some(format!(
                        "bin data {} was missing, so the image resource could not be loaded",
                        picture.image_attr.bin_data_id
                    )),
                    source: Some("rhwp".to_string()),
                })
            }
        }
    }

    fn warn_unsupported_picture_transform(&mut self, picture: &Picture) {
        let mut details = Vec::new();

        let border_line_type = (picture.border_attr.attr & 0x3f) as u8;
        if border_line_type != 0 && !picture_border_line_type_is_modeled(border_line_type) {
            self.add_warning_once(&format!(
                "rhwp picture border line type {border_line_type} is not directly modeled; hwp-convert approximated it as a solid border."
            ));
        }

        if picture.image_attr.effect == RhwpImageEffect::BlackWhite {
            self.add_warning_once(
                "rhwp picture BlackWhite effect was represented as a grayscale approximation because Image IR does not distinguish threshold black-and-white.",
            );
        }
        if picture.image_attr.effect == RhwpImageEffect::Pattern8x8 {
            self.add_warning_once(
                "rhwp picture Pattern8x8 effect was preserved in Image IR; semantic exporters currently use the unfiltered source bytes.",
            );
        }

        if map_picture_crop(picture).is_some() {
            self.add_warning_once(&format!(
                "rhwp picture crop ({}/{}/{}/{}) was preserved in Image IR; semantic image exporters currently use the uncropped source bytes.",
                picture.crop.left,
                picture.crop.top,
                picture.crop.right,
                picture.crop.bottom
            ));
        } else if !picture_crop_is_empty(picture) {
            details.push(format!(
                "invalid_crop={}/{}/{}/{}",
                picture.crop.left, picture.crop.top, picture.crop.right, picture.crop.bottom
            ));
        }
        if picture.shape_attr.render_b.abs() > f64::EPSILON
            || picture.shape_attr.render_c.abs() > f64::EPSILON
        {
            details.push(format!(
                "affine_shear_or_rotation={}/{}",
                picture.shape_attr.render_b, picture.shape_attr.render_c
            ));
        }
        let has_unmodeled_layout = !picture.common.treat_as_char
            || picture.common.horizontal_offset != 0
            || picture.common.vertical_offset != 0
            || picture.common.text_wrap != rhwp::model::shape::TextWrap::Square
            || picture.common.vert_rel_to != rhwp::model::shape::VertRelTo::Paper
            || picture.common.horz_rel_to != rhwp::model::shape::HorzRelTo::Paper
            || picture.common.vert_align != rhwp::model::shape::VertAlign::Top
            || picture.common.horz_align != rhwp::model::shape::HorzAlign::Left;
        if has_unmodeled_layout {
            self.add_warning_once(&format!(
                "rhwp picture layout (treat_as_char:{},wrap:{:?},vertical:{:?}/{:?}/{},horizontal:{:?}/{:?}/{}) was preserved in Image IR; semantic exporters currently linearize the image without floating placement.",
                picture.common.treat_as_char,
                picture.common.text_wrap,
                picture.common.vert_rel_to,
                picture.common.vert_align,
                picture.common.vertical_offset,
                picture.common.horz_rel_to,
                picture.common.horz_align,
                picture.common.horizontal_offset
            ));
        }
        // GrayScale is modeled directly. BlackWhite is retained as a visible
        // grayscale approximation with the warning above.
        if picture.image_attr.brightness != 0 || picture.image_attr.contrast != 0 {
            self.add_warning_once(&format!(
                "rhwp picture brightness/contrast (brightness:{},contrast:{}) was preserved in Image IR; semantic exporters currently use the unadjusted source bytes.",
                picture.image_attr.brightness, picture.image_attr.contrast
            ));
        }
        if picture.border_opacity != 0 {
            details.push(format!("border_opacity={}", picture.border_opacity));
        }
        if picture.padding.left < 0
            || picture.padding.right < 0
            || picture.padding.top < 0
            || picture.padding.bottom < 0
        {
            details.push(format!(
                "negative_padding={}/{}/{}/{}",
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
            "rhwp exposed picture visual properties that Image IR does not yet model; hwp-convert preserved the original image bytes without applying {}.",
            details.join(", ")
        ));
    }

    fn map_equation(&mut self, equation: &RhwpEquation) -> Equation {
        let content = non_empty_string(&equation.script);
        Equation {
            kind: EquationKind::PlainText,
            fallback_text: content.clone().or_else(|| Some("[equation]".to_string())),
            content,
            resource_id: None,
            font_size_pt: (equation.font_size > 0)
                .then(|| LengthPt(equation.font_size as f32 / 100.0)),
            color: color_ref_to_color_option(equation.color),
            baseline_pt: (equation.baseline != 0)
                .then(|| LengthPt(equation.baseline as f32 / 100.0)),
            font_family: non_empty_string(&equation.font_name),
            version: non_empty_string(&equation.version_info),
            width: hwp_units_to_px_option(equation.common.width),
            height: hwp_units_to_px_option(equation.common.height),
            offset_x: hwp_units_to_px_option(equation.common.horizontal_offset),
            offset_y: hwp_units_to_px_option(equation.common.vertical_offset),
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
        let common = shape.common();
        let drawing = shape.drawing();
        let border = drawing.and_then(|drawing| map_shape_border_line(&drawing.border_line));
        let background_color = drawing.and_then(map_shape_background_color);
        let text_box = drawing.and_then(|drawing| drawing.text_box.as_ref());
        let drawing_details = drawing
            .map(shape_unmodeled_presentation_details)
            .unwrap_or_else(|| "drawing details unavailable".to_string());
        self.add_warning_once(&format!(
            "rhwp shape {kind:?} remains a semantic placeholder; hwp-convert preserved kind/text, basic size/offset, simple border, and solid fill when available (z_order={}, {drawing_details}).",
            common.z_order
        ));
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
            border,
            background_color,
            rotation_degrees: (shape.shape_attr().rotation_angle != 0)
                .then(|| shape.shape_attr().rotation_angle as f32),
            flip_horizontal: shape.shape_attr().horz_flip.then_some(true),
            flip_vertical: shape.shape_attr().vert_flip.then_some(true),
            text_vertical_align: text_box
                .and_then(|text_box| map_vertical_align(text_box.vertical_align)),
            padding_top: text_box
                .and_then(|text_box| i16_hwp_units_to_px_option(text_box.margin_top)),
            padding_right: text_box
                .and_then(|text_box| i16_hwp_units_to_px_option(text_box.margin_right)),
            padding_bottom: text_box
                .and_then(|text_box| i16_hwp_units_to_px_option(text_box.margin_bottom)),
            padding_left: text_box
                .and_then(|text_box| i16_hwp_units_to_px_option(text_box.margin_left)),
            width: hwp_units_to_px_option(common.width),
            height: hwp_units_to_px_option(common.height),
            offset_x: hwp_units_to_px_option(common.horizontal_offset),
            offset_y: hwp_units_to_px_option(common.vertical_offset),
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
                        ..Default::default()
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

    fn map_style_sheet(&mut self) -> StyleSheet {
        let mut style_sheet = StyleSheet::default();

        for index in 0..self.source.doc_info.styles.len() {
            let style = &self.source.doc_info.styles[index];
            let name = style_name(style);
            let char_shape_id = style.char_shape_id as u32;
            let para_shape_id = style.para_shape_id;

            style_sheet.text_styles.push(NamedTextStyle {
                id: TextStyleId(text_style_key(index)),
                name: name.clone(),
                style: self
                    .map_text_style_by_char_shape_id_or_warn(char_shape_id, "style sheet")
                    .unwrap_or_default(),
            });

            style_sheet.paragraph_styles.push(NamedParagraphStyle {
                id: ParagraphStyleId(paragraph_style_key(index)),
                name,
                style: self.map_paragraph_style_by_id(para_shape_id, "style sheet"),
            });
        }

        style_sheet
    }

    fn map_text_style(&mut self, char_shape: &RhwpCharShape, context: &str) -> TextStyle {
        self.warn_unmodeled_text_style(char_shape);
        self.warn_nonuniform_text_metrics(char_shape, context);
        let font_width_percent = uniform_u8_percent(&char_shape.ratios, 50..=200);
        let letter_spacing_percent = uniform_i8_percent(&char_shape.spacings, -50..=50);
        let relative_size_percent = uniform_u8_percent(&char_shape.relative_sizes, 10..=250);
        let vertical_offset_percent = uniform_i8_percent(&char_shape.char_offsets, -100..=100);
        let font_size_pt = self
            .map_font_size_pt(char_shape.base_size, context)
            .map(|size| match relative_size_percent {
                Some(relative_size) => LengthPt(size.0 * relative_size.0 / 100.0),
                None => size,
            });

        TextStyle {
            bold: char_shape.bold,
            italic: char_shape.italic,
            underline: char_shape.underline_type != RhwpUnderlineType::None,
            strike: char_shape.strikethrough,
            superscript: char_shape.superscript,
            subscript: char_shape.subscript,
            emphasis_dot: char_shape.emphasis_dot != 0,
            emboss: char_shape.emboss,
            engrave: char_shape.engrave,
            outline: char_shape.outline_type != 0,
            shadow: char_shape.shadow_type != 0,
            font_family: self.lookup_font_family(char_shape, context),
            font_size_pt,
            color: color_ref_to_color_option(char_shape.text_color),
            background_color: color_ref_to_color_option(char_shape.shade_color),
            underline_color: color_ref_to_color_option(char_shape.underline_color),
            strike_color: color_ref_to_color_option(char_shape.strike_color),
            underline_style: (char_shape.underline_type != RhwpUnderlineType::None)
                .then(|| map_text_decoration_style(char_shape.underline_shape)),
            strike_style: char_shape
                .strikethrough
                .then(|| map_text_decoration_style(char_shape.strike_shape)),
            underline_above: char_shape.underline_type == RhwpUnderlineType::Top,
            font_width_percent,
            letter_spacing_percent,
            relative_size_percent,
            vertical_offset_percent,
            kerning: char_shape.kerning,
        }
    }

    fn map_font_size_pt(&mut self, value: i32, context: &str) -> Option<LengthPt> {
        const MAX_FONT_SIZE_HWPUNIT: i32 = 4096 * 100;

        if value == 0 {
            return None;
        }
        if !(1..=MAX_FONT_SIZE_HWPUNIT).contains(&value) {
            self.add_warning_once(&format!(
                "rhwp {context} exposed invalid font size {value} HWPUNIT; hwp-convert omitted the font size."
            ));
            return None;
        }

        Some(LengthPt(value as f32 / 100.0))
    }

    fn warn_unmodeled_text_style(&mut self, char_shape: &RhwpCharShape) {
        if percent_values_have_invalid_u8(&char_shape.ratios, 50..=200) {
            self.add_warning_once(&format!(
                "rhwp text style used invalid horizontal ratios {:?}; TextStyle omitted the out-of-range values.",
                char_shape.ratios
            ));
        }
        if percent_values_have_invalid_i8(&char_shape.spacings, -50..=50) {
            self.add_warning_once(&format!(
                "rhwp text style used invalid character spacing {:?}; TextStyle omitted the out-of-range values.",
                char_shape.spacings
            ));
        }
        if percent_values_have_invalid_u8(&char_shape.relative_sizes, 10..=250) {
            self.add_warning_once(&format!(
                "rhwp text style used invalid relative sizes {:?}; TextStyle omitted the out-of-range values.",
                char_shape.relative_sizes
            ));
        }
        if percent_values_have_invalid_i8(&char_shape.char_offsets, -100..=100) {
            self.add_warning_once(&format!(
                "rhwp text style used invalid character offsets {:?}; TextStyle omitted the out-of-range values.",
                char_shape.char_offsets
            ));
        }
        if char_shape.underline_type != RhwpUnderlineType::None
            && decoration_shape_is_approximated(char_shape.underline_shape)
        {
            self.add_warning_once(&format!(
                "rhwp text style used underline shape {}; hwp-convert preserved its closest CSS decoration style but exact line geometry is approximated.",
                char_shape.underline_shape
            ));
        }
        if char_shape.strikethrough && decoration_shape_is_approximated(char_shape.strike_shape) {
            self.add_warning_once(&format!(
                "rhwp text style used strike shape {}; hwp-convert preserved its closest CSS decoration style but exact line geometry is approximated.",
                char_shape.strike_shape
            ));
        }
        if char_shape.underline_type != RhwpUnderlineType::None
            && char_shape.strikethrough
            && map_text_decoration_style(char_shape.underline_shape)
                != map_text_decoration_style(char_shape.strike_shape)
        {
            self.add_warning_once(
                "rhwp text style used different simultaneous underline and strike decoration styles; JSON preserves both, while HTML CSS can display only one shared decoration style and prefers the underline style.",
            );
        }
        if char_shape.emphasis_dot > 1 {
            self.add_warning_once(&format!(
                "rhwp text style used emphasis mark type {}; TextStyle approximated it as a generic dot emphasis.",
                char_shape.emphasis_dot
            ));
        }
        if char_shape.outline_type != 0 {
            self.add_warning_once(&format!(
                "rhwp text style used outline type {}; HTML approximates it with a uniform text stroke.",
                char_shape.outline_type
            ));
        }
        if char_shape.shadow_type != 0 || char_shape.emboss || char_shape.engrave {
            self.add_warning_once(
                "rhwp text effect details such as shadow type, offsets, color, emboss, or engrave are only approximated by generic HTML text shadows.",
            );
        }
    }

    fn warn_nonuniform_text_metrics(&mut self, char_shape: &RhwpCharShape, context: &str) {
        if percent_values_are_nonuniform_u8(&char_shape.ratios, 50..=200) {
            self.add_warning_once(&format!(
                "rhwp {context} used script-specific horizontal ratios {:?}; this single named TextStyle preserved only its primary-script value, while paragraph runs retain per-script values.",
                char_shape.ratios
            ));
        }
        if percent_values_are_nonuniform_i8(&char_shape.spacings, -50..=50) {
            self.add_warning_once(&format!(
                "rhwp {context} used script-specific character spacing {:?}; this single named TextStyle preserved only its primary-script value, while paragraph runs retain per-script values.",
                char_shape.spacings
            ));
        }
        if percent_values_are_nonuniform_u8(&char_shape.relative_sizes, 10..=250) {
            self.add_warning_once(&format!(
                "rhwp {context} used script-specific relative sizes {:?}; this single named TextStyle preserved only its primary-script value, while paragraph runs retain per-script values.",
                char_shape.relative_sizes
            ));
        }
        if percent_values_are_nonuniform_i8(&char_shape.char_offsets, -100..=100) {
            self.add_warning_once(&format!(
                "rhwp {context} used script-specific character offsets {:?}; this single named TextStyle preserved only its primary-script value, while paragraph runs retain per-script values.",
                char_shape.char_offsets
            ));
        }
    }

    fn map_paragraph_style_by_id(&mut self, para_shape_id: u16, context: &str) -> ParagraphStyle {
        if let Some(para_shape) = self.lookup_para_shape(para_shape_id).cloned() {
            let style = self.map_paragraph_style(&para_shape);
            self.warn_unmodeled_paragraph_style(&para_shape);
            return style;
        }

        if !self.source.doc_info.para_shapes.is_empty() || para_shape_id != 0 {
            self.add_warning_once(&format!(
                "rhwp {context} referenced missing para shape id {para_shape_id}; hwp-convert used fallback paragraph style."
            ));
        }

        ParagraphStyle::default()
    }

    fn warn_unmodeled_paragraph_style(&mut self, para_shape: &RhwpParaShape) {
        if matches!(
            para_shape.alignment,
            RhwpAlignment::Distribute | RhwpAlignment::Split
        ) {
            self.add_warning_once(&format!(
                "rhwp paragraph alignment {:?} is not directly modeled; hwp-convert approximated it as justify.",
                para_shape.alignment
            ));
        }

        match para_shape.line_spacing_type {
            rhwp::model::style::LineSpacingType::SpaceOnly
            | rhwp::model::style::LineSpacingType::Minimum => {
                self.add_warning_once(&format!(
                    "rhwp paragraph line spacing mode {:?} is not directly modeled; hwp-convert approximated its numeric value as a fixed point line height.",
                    para_shape.line_spacing_type
                ));
            }
            rhwp::model::style::LineSpacingType::Fixed
            | rhwp::model::style::LineSpacingType::Percent => {}
        }

        if para_shape.tab_def_id != 0 {
            self.add_warning_once(&format!(
                "rhwp paragraph referenced tab definition id {}; custom tab stops are not modeled and were omitted.",
                para_shape.tab_def_id
            ));
        }
        if para_shape.border_spacing.iter().any(|spacing| *spacing < 0) {
            self.add_warning_once(&format!(
                "rhwp paragraph border spacing {:?} contained negative HWPUNIT values; hwp-convert omitted the negative sides.",
                para_shape.border_spacing
            ));
        }
    }

    fn map_paragraph_style(&mut self, para_shape: &RhwpParaShape) -> ParagraphStyle {
        let mut style = ParagraphStyle {
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
                line_percent: match para_shape.line_spacing_type {
                    rhwp::model::style::LineSpacingType::Percent => {
                        let value = if para_shape.line_spacing_v2 > 0 {
                            para_shape.line_spacing_v2 as f32
                        } else {
                            para_shape.line_spacing.max(0) as f32
                        };
                        (value > 0.0).then_some(Percent(value))
                    }
                    _ => None,
                },
            },
            indent: crate::ir::Indent {
                left_pt: i32_hwp_units_to_pt_option(para_shape.margin_left),
                right_pt: i32_hwp_units_to_pt_option(para_shape.margin_right),
                first_line_pt: i32_hwp_units_to_pt_option(para_shape.indent),
            },
            widow_orphan: paragraph_layout_flag(para_shape, 16, 5),
            keep_with_next: paragraph_layout_flag(para_shape, 17, 6),
            keep_lines: paragraph_layout_flag(para_shape, 18, 7),
            page_break_before: paragraph_layout_flag(para_shape, 19, 8),
            ..Default::default()
        };

        if para_shape.border_fill_id != 0 {
            style.background_color = self
                .border_fill_background_color(para_shape.border_fill_id, "paragraph background");
            let [border_left, border_right, border_top, border_bottom] =
                self.map_borders(para_shape.border_fill_id, "paragraph borders");
            style.border_top = border_top;
            style.border_right = border_right;
            style.border_bottom = border_bottom;
            style.border_left = border_left;
            style.padding_left_pt = i16_hwp_units_to_pt_option(para_shape.border_spacing[0]);
            style.padding_right_pt = i16_hwp_units_to_pt_option(para_shape.border_spacing[1]);
            style.padding_top_pt = i16_hwp_units_to_pt_option(para_shape.border_spacing[2]);
            style.padding_bottom_pt = i16_hwp_units_to_pt_option(para_shape.border_spacing[3]);
        }

        style
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

    fn map_text_style_by_char_shape_id_or_warn(
        &mut self,
        char_shape_id: u32,
        context: &str,
    ) -> Option<TextStyle> {
        match self.lookup_char_shape(char_shape_id).cloned() {
            Some(char_shape) => Some(self.map_text_style(&char_shape, context)),
            None => {
                if !self.source.doc_info.char_shapes.is_empty() || char_shape_id != 0 {
                    self.add_warning_once(&format!(
                        "rhwp {context} referenced missing char shape id {char_shape_id}; hwp-convert used fallback text style."
                    ));
                }
                None
            }
        }
    }

    fn map_text_style_for_language_by_char_shape_id_or_warn(
        &mut self,
        char_shape_id: u32,
        language_index: usize,
        context: &str,
    ) -> Option<TextStyle> {
        match self.lookup_char_shape(char_shape_id).cloned() {
            Some(char_shape) => {
                Some(self.map_text_style_for_language(&char_shape, language_index, context))
            }
            None => {
                if !self.source.doc_info.char_shapes.is_empty() || char_shape_id != 0 {
                    self.add_warning_once(&format!(
                        "rhwp {context} referenced missing char shape id {char_shape_id}; hwp-convert used fallback text style."
                    ));
                }
                None
            }
        }
    }

    fn map_text_style_for_language(
        &mut self,
        char_shape: &RhwpCharShape,
        language_index: usize,
        context: &str,
    ) -> TextStyle {
        let language_index = language_index.min(6);
        self.warn_unmodeled_text_style(char_shape);
        let font_width_percent = u8_percent_at(&char_shape.ratios, language_index, 50..=200);
        let letter_spacing_percent = i8_percent_at(&char_shape.spacings, language_index, -50..=50);
        let relative_size_percent =
            u8_percent_at(&char_shape.relative_sizes, language_index, 10..=250);
        let vertical_offset_percent =
            i8_percent_at(&char_shape.char_offsets, language_index, -100..=100);
        let font_size_pt = self
            .map_font_size_pt(char_shape.base_size, context)
            .map(|size| match relative_size_percent {
                Some(relative_size) => LengthPt(size.0 * relative_size.0 / 100.0),
                None => size,
            });

        TextStyle {
            bold: char_shape.bold,
            italic: char_shape.italic,
            underline: char_shape.underline_type != RhwpUnderlineType::None,
            strike: char_shape.strikethrough,
            superscript: char_shape.superscript,
            subscript: char_shape.subscript,
            emphasis_dot: char_shape.emphasis_dot != 0,
            emboss: char_shape.emboss,
            engrave: char_shape.engrave,
            outline: char_shape.outline_type != 0,
            shadow: char_shape.shadow_type != 0,
            font_family: self.lookup_font_family_for_language(char_shape, language_index, context),
            font_size_pt,
            color: color_ref_to_color_option(char_shape.text_color),
            background_color: color_ref_to_color_option(char_shape.shade_color),
            underline_color: color_ref_to_color_option(char_shape.underline_color),
            strike_color: color_ref_to_color_option(char_shape.strike_color),
            underline_style: (char_shape.underline_type != RhwpUnderlineType::None)
                .then(|| map_text_decoration_style(char_shape.underline_shape)),
            strike_style: char_shape
                .strikethrough
                .then(|| map_text_decoration_style(char_shape.strike_shape)),
            underline_above: char_shape.underline_type == RhwpUnderlineType::Top,
            font_width_percent,
            letter_spacing_percent,
            relative_size_percent,
            vertical_offset_percent,
            kerning: char_shape.kerning,
        }
    }

    fn lookup_para_shape(&self, para_shape_id: u16) -> Option<&RhwpParaShape> {
        self.source.doc_info.para_shapes.get(para_shape_id as usize)
    }

    fn lookup_font_family(&mut self, char_shape: &RhwpCharShape, context: &str) -> Option<String> {
        let mut selected = None;
        let mut distinct_names = Vec::new();

        for (language_index, font_id) in char_shape.font_ids.iter().enumerate() {
            let Some(group) = self.source.doc_info.font_faces.get(language_index) else {
                if *font_id != 0 {
                    self.add_warning_once(&format!(
                        "rhwp {context} referenced missing font face group {language_index} font id {font_id}; hwp-convert used an available fallback font family or default font style."
                    ));
                }
                continue;
            };
            let Some(font) = group.get(*font_id as usize) else {
                if *font_id != 0 || !group.is_empty() {
                    self.add_warning_once(&format!(
                        "rhwp {context} referenced missing font id {font_id} in font face group {language_index}; hwp-convert used an available fallback font family or default font style."
                    ));
                }
                continue;
            };
            if let Some(name) = non_empty_string(&font.name) {
                if selected.is_none() {
                    selected = Some(name.clone());
                }
                if !distinct_names.contains(&name) {
                    distinct_names.push(name);
                }
            }
        }

        if distinct_names.len() > 1 {
            self.add_warning_once(&format!(
                "rhwp {context} used multiple script-specific font families ({}); TextStyle preserved only the first available family.",
                distinct_names.join(", ")
            ));
        }

        selected
    }

    fn lookup_font_family_for_language(
        &mut self,
        char_shape: &RhwpCharShape,
        language_index: usize,
        context: &str,
    ) -> Option<String> {
        let font_id = char_shape.font_ids[language_index];
        match self.source.doc_info.font_faces.get(language_index) {
            Some(group) => match group.get(font_id as usize) {
                Some(font) => {
                    if let Some(name) = non_empty_string(&font.name) {
                        return Some(name);
                    }
                }
                None if font_id != 0 || !group.is_empty() => {
                    self.add_warning_once(&format!(
                        "rhwp {context} referenced missing font id {font_id} in font face group {language_index}; hwp-convert used an available fallback font family or default font style."
                    ));
                }
                None => {}
            },
            None if font_id != 0 => {
                self.add_warning_once(&format!(
                    "rhwp {context} referenced missing font face group {language_index} font id {font_id}; hwp-convert used an available fallback font family or default font style."
                ));
            }
            None => {}
        }

        char_shape
            .font_ids
            .iter()
            .enumerate()
            .find_map(|(fallback_index, fallback_id)| {
                self.source
                    .doc_info
                    .font_faces
                    .get(fallback_index)
                    .and_then(|group| group.get(*fallback_id as usize))
                    .and_then(|font| non_empty_string(&font.name))
            })
    }

    fn border_fill_background_color(
        &mut self,
        border_fill_id: u16,
        context: &str,
    ) -> Option<Color> {
        match self.lookup_border_fill(border_fill_id).cloned() {
            Some(border_fill) => {
                self.warn_unmodeled_border_fill(&border_fill, context);
                border_fill_background_color(&border_fill)
            }
            None => {
                if border_fill_id != 0 {
                    self.warn_missing_border_fill(border_fill_id, context);
                }
                None
            }
        }
    }

    fn warn_unmodeled_border_fill(&mut self, border_fill: &RhwpBorderFill, context: &str) {
        match border_fill.fill.fill_type {
            RhwpFillType::None => {}
            RhwpFillType::Solid => match border_fill.fill.solid.as_ref() {
                Some(solid) if solid.pattern_type > 0 => {
                    self.add_warning_once(&format!(
                        "rhwp {context} used solid fill pattern type {}; hwp-convert approximated it with the pattern background color.",
                        solid.pattern_type
                    ));
                }
                Some(_) => {}
                None => {
                    self.add_warning_once(&format!(
                        "rhwp {context} declared a solid fill without solid fill data; hwp-convert omitted the background fill."
                    ));
                }
            },
            RhwpFillType::Gradient => {
                self.add_warning_once(&format!(
                    "rhwp {context} used a gradient fill that semantic IR does not model; hwp-convert omitted the background fill."
                ));
            }
            RhwpFillType::Image => {
                self.add_warning_once(&format!(
                    "rhwp {context} used an image fill that semantic IR does not model; hwp-convert omitted the background fill."
                ));
            }
        }

        if !matches!(border_fill.fill.alpha, 0 | 255) {
            self.add_warning_once(&format!(
                "rhwp {context} used fill opacity {}; hwp-convert did not apply that opacity.",
                border_fill.fill.alpha
            ));
        }
    }

    fn warn_negative_table_padding(&mut self, padding: &rhwp::model::Padding, source: &str) {
        if padding.left >= 0 && padding.right >= 0 && padding.top >= 0 && padding.bottom >= 0 {
            return;
        }

        self.add_warning_once(&format!(
            "rhwp table cell {source} padding contained negative HWPUNIT values left={}, right={}, top={}, bottom={}; hwp-convert omitted the negative sides.",
            padding.left, padding.right, padding.top, padding.bottom
        ));
    }

    /// Map a border fill's four sides to IR borders, in rhwp order
    /// `[left, right, top, bottom]`.
    fn map_borders(&mut self, border_fill_id: u16, context: &str) -> [Option<Border>; 4] {
        match self.lookup_border_fill(border_fill_id).cloned() {
            Some(border_fill) => {
                for line in &border_fill.borders {
                    if table_border_line_type_is_approximated(line.line_type) {
                        self.add_warning_once(&format!(
                            "rhwp {context} used border line type {:?} that is not directly modeled; hwp-convert approximated it as a simpler CSS border style.",
                            line.line_type
                        ));
                    }
                }

                [
                    map_border_line(&border_fill.borders[0]),
                    map_border_line(&border_fill.borders[1]),
                    map_border_line(&border_fill.borders[2]),
                    map_border_line(&border_fill.borders[3]),
                ]
            }
            None => {
                if border_fill_id != 0 {
                    self.warn_missing_border_fill(border_fill_id, context);
                }
                [None, None, None, None]
            }
        }
    }

    fn warn_missing_border_fill(&mut self, border_fill_id: u16, context: &str) {
        self.add_warning_once(&format!(
            "rhwp {context} referenced missing border fill id {border_fill_id}; hwp-convert used default border/fill style."
        ));
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
    char_shape_id: Option<u32>,
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

    fn counters(&self, numbering_id: u16) -> Option<&[u32; 7]> {
        self.counters.get(&numbering_id)
    }
}

fn expand_ordered_marker(format: &str, counters: &[u32; 7], numbering: &RhwpNumbering) -> String {
    let mut result = String::new();
    let mut chars = format.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch == '^'
            && let Some(digit) = chars.peek().copied()
            && ('1'..='7').contains(&digit)
        {
            chars.next();
            let index = (digit as u8 - b'1') as usize;
            let number = if counters[index] > 0 {
                counters[index]
            } else {
                numbering_level_start(numbering, index)
            };
            result.push_str(&format_numbering_value(
                number,
                numbering.heads[index].number_format,
            ));
            continue;
        }
        result.push(ch);
    }

    result
}

fn numbering_level_start(numbering: &RhwpNumbering, index: usize) -> u32 {
    let level_start = numbering.level_start_numbers[index];
    if level_start > 0 {
        level_start
    } else if index == 0 && numbering.start_number > 0 {
        numbering.start_number as u32
    } else {
        1
    }
}

fn format_numbering_value(number: u32, format_code: u8) -> String {
    let Some(format) = numbering_format(format_code) else {
        return number.to_string();
    };
    let Ok(number) = u16::try_from(number) else {
        return number.to_string();
    };
    format_rhwp_number(number, format)
}

fn numbering_format(format_code: u8) -> Option<RhwpNumberFormat> {
    match format_code {
        0 => Some(RhwpNumberFormat::Digit),
        1 => Some(RhwpNumberFormat::CircledDigit),
        2 => Some(RhwpNumberFormat::RomanUpper),
        3 => Some(RhwpNumberFormat::RomanLower),
        4 => Some(RhwpNumberFormat::LatinUpper),
        5 => Some(RhwpNumberFormat::LatinLower),
        8 => Some(RhwpNumberFormat::HangulGaNaDa),
        12 => Some(RhwpNumberFormat::HangulNumber),
        13 => Some(RhwpNumberFormat::HanjaNumber),
        _ => None,
    }
}

fn numbering_marker_level_references(format: &str) -> Vec<usize> {
    let mut referenced = [false; 7];
    let mut chars = format.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '^'
            && let Some(digit) = chars.peek().copied()
            && ('1'..='7').contains(&digit)
        {
            chars.next();
            referenced[(digit as u8 - b'1') as usize] = true;
        }
    }

    referenced
        .iter()
        .enumerate()
        .filter_map(|(index, is_referenced)| is_referenced.then_some(index))
        .collect()
}

fn split_text_by_language(text: &str) -> Vec<(usize, String)> {
    let chars = text.chars().collect::<Vec<_>>();
    if chars.is_empty() {
        return Vec::new();
    }

    let initial_language = chars
        .iter()
        .copied()
        .find(|ch| !is_language_neutral(*ch))
        .map(rhwp::renderer::style_resolver::detect_lang_category)
        .unwrap_or(0);
    let mut current_language = initial_language;
    let mut current_start = 0usize;
    let mut runs = Vec::new();

    for (index, ch) in chars.iter().copied().enumerate() {
        if is_language_neutral(ch) {
            continue;
        }
        let language = rhwp::renderer::style_resolver::detect_lang_category(ch);
        if language == current_language {
            continue;
        }

        if index > current_start {
            runs.push((
                current_language,
                chars[current_start..index].iter().collect(),
            ));
        }
        current_language = language;
        current_start = index;
    }

    if current_start < chars.len() {
        runs.push((current_language, chars[current_start..].iter().collect()));
    }
    runs
}

fn is_language_neutral(ch: char) -> bool {
    matches!(
        ch as u32,
        0x0000..=0x0020
            | 0x0021..=0x002F
            | 0x003A..=0x0040
            | 0x005B..=0x0060
            | 0x007B..=0x007F
            | 0x00A0..=0x00BF
    )
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

fn infer_control_text_positions(paragraph: &RhwpParagraph) -> Vec<Option<usize>> {
    let mut positions = vec![None; paragraph.controls.len()];
    if paragraph.controls.is_empty() {
        return positions;
    }

    let chars = paragraph.text.chars().collect::<Vec<_>>();
    for range in &paragraph.field_ranges {
        if range.control_idx < positions.len()
            && range.start_char_idx <= range.end_char_idx
            && range.end_char_idx <= chars.len()
        {
            positions[range.control_idx] = Some(range.start_char_idx);
        }
    }

    if chars.is_empty() || paragraph.char_offsets.len() != chars.len() {
        return positions;
    }
    if paragraph
        .char_offsets
        .windows(2)
        .any(|window| window[0] >= window[1])
    {
        return positions;
    }

    let mut candidates = Vec::new();
    let mut ambiguous_gap = false;
    push_control_gap_candidates(
        &mut candidates,
        0,
        paragraph.char_offsets[0] as usize,
        &mut ambiguous_gap,
    );

    for (index, window) in paragraph.char_offsets.windows(2).enumerate() {
        let current_offset = window[0] as usize;
        let next_offset = window[1] as usize;
        let current_width = source_text_char_width(chars[index]);
        let Some(gap) = next_offset.checked_sub(current_offset + current_width) else {
            return positions;
        };
        push_control_gap_candidates(&mut candidates, index + 1, gap, &mut ambiguous_gap);
    }

    if paragraph.char_count > 0 {
        let content_end = paragraph.char_count.saturating_sub(1) as usize;
        let last_index = chars.len() - 1;
        let last_end =
            paragraph.char_offsets[last_index] as usize + source_text_char_width(chars[last_index]);
        let Some(gap) = content_end.checked_sub(last_end) else {
            return positions;
        };
        push_control_gap_candidates(&mut candidates, chars.len(), gap, &mut ambiguous_gap);
    }

    for range in &paragraph.field_ranges {
        if range.control_idx >= positions.len()
            || range.start_char_idx > range.end_char_idx
            || range.end_char_idx > chars.len()
        {
            continue;
        }
        if !remove_control_candidate(&mut candidates, range.start_char_idx)
            || !remove_control_candidate(&mut candidates, range.end_char_idx)
        {
            ambiguous_gap = true;
        }
    }

    let unmapped = positions
        .iter()
        .enumerate()
        .filter_map(|(index, position)| position.is_none().then_some(index))
        .collect::<Vec<_>>();
    if !ambiguous_gap && candidates.len() == unmapped.len() {
        for (control_index, position) in unmapped.into_iter().zip(candidates) {
            positions[control_index] = Some(position);
        }
    }

    positions
}

fn push_control_gap_candidates(
    candidates: &mut Vec<usize>,
    text_position: usize,
    gap: usize,
    ambiguous: &mut bool,
) {
    if gap == 0 {
        return;
    }
    if !gap.is_multiple_of(8) {
        *ambiguous = true;
        return;
    }
    candidates.extend(std::iter::repeat_n(text_position, gap / 8));
}

fn remove_control_candidate(candidates: &mut Vec<usize>, position: usize) -> bool {
    let Some(index) = candidates
        .iter()
        .position(|candidate| *candidate == position)
    else {
        return false;
    };
    candidates.remove(index);
    true
}

fn source_text_char_width(ch: char) -> usize {
    if ch == '\t' {
        8
    } else if ch.len_utf16() == 2 {
        2
    } else {
        1
    }
}

fn uniform_u8_percent(
    values: &[u8; 7],
    valid_range: std::ops::RangeInclusive<u8>,
) -> Option<Percent> {
    let first = values[0];
    (values.iter().all(|value| *value == first)
        && !matches!(first, 0 | 100)
        && valid_range.contains(&first))
    .then_some(Percent(first as f32))
}

fn u8_percent_at(
    values: &[u8; 7],
    index: usize,
    valid_range: std::ops::RangeInclusive<u8>,
) -> Option<Percent> {
    let value = values[index.min(6)];
    (!matches!(value, 0 | 100) && valid_range.contains(&value)).then_some(Percent(value as f32))
}

fn uniform_i8_percent(
    values: &[i8; 7],
    valid_range: std::ops::RangeInclusive<i8>,
) -> Option<Percent> {
    let first = values[0];
    (first != 0 && values.iter().all(|value| *value == first) && valid_range.contains(&first))
        .then_some(Percent(first as f32))
}

fn i8_percent_at(
    values: &[i8; 7],
    index: usize,
    valid_range: std::ops::RangeInclusive<i8>,
) -> Option<Percent> {
    let value = values[index.min(6)];
    (value != 0 && valid_range.contains(&value)).then_some(Percent(value as f32))
}

fn percent_values_have_invalid_u8(
    values: &[u8; 7],
    valid_range: std::ops::RangeInclusive<u8>,
) -> bool {
    values
        .iter()
        .any(|value| !matches!(*value, 0 | 100) && !valid_range.contains(value))
}

fn percent_values_have_invalid_i8(
    values: &[i8; 7],
    valid_range: std::ops::RangeInclusive<i8>,
) -> bool {
    values
        .iter()
        .any(|value| *value != 0 && !valid_range.contains(value))
}

fn percent_values_are_nonuniform_u8(
    values: &[u8; 7],
    valid_range: std::ops::RangeInclusive<u8>,
) -> bool {
    values.iter().any(|value| !matches!(*value, 0 | 100))
        && !percent_values_have_invalid_u8(values, valid_range)
        && values.iter().any(|value| *value != values[0])
}

fn percent_values_are_nonuniform_i8(
    values: &[i8; 7],
    valid_range: std::ops::RangeInclusive<i8>,
) -> bool {
    values.iter().any(|value| *value != 0)
        && !percent_values_have_invalid_i8(values, valid_range)
        && values.iter().any(|value| *value != values[0])
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

fn i16_hwp_units_to_px_option(value: i16) -> Option<LengthPx> {
    if value <= 0 {
        None
    } else {
        hwp_units_to_px_option(value as u32)
    }
}

/// Map a single rhwp border line to an IR cell border. `None` line type means
/// the side has no border.
fn map_border_line(line: &RhwpBorderLine) -> Option<Border> {
    if line.line_type == RhwpBorderLineType::None {
        return None;
    }

    Some(Border {
        width: border_width_index_to_px(line.width),
        style: map_border_line_type(line.line_type),
        color: color_ref_to_color_option(line.color),
    })
}

/// Map an rhwp picture's uniform border to an IR border. The low six bits of
/// `border_attr` contain the HWP line type; zero means no border.
fn map_image_border(picture: &Picture) -> Option<Border> {
    let line_type = (picture.border_attr.attr & 0x3f) as u8;
    if line_type == 0 {
        return None;
    }

    let width = if picture.border_width > 0 {
        hwp_units_to_px_option(picture.border_width as u32)?
    } else {
        // HWP specifies 0.1 mm as the default width for an active picture line.
        LengthPx(96.0 / 254.0)
    };
    let style = match line_type {
        2 | 4..=6 => BorderStyle::Dashed,
        3 | 7 => BorderStyle::Dotted,
        8..=11 | 13 => BorderStyle::Double,
        _ => BorderStyle::Solid,
    };

    Some(Border {
        width,
        style,
        color: color_ref_to_color_option(picture.border_color),
    })
}

/// Map a drawing object's uniform border when its HWP line type has a direct
/// representation in the semantic IR. HWP stores a zero width for active
/// drawing borders as the standard 0.1 mm width.
fn map_shape_border_line(line: &RhwpShapeBorderLine) -> Option<Border> {
    let line_type = (line.attr & 0x3f) as u8;
    if !picture_border_line_type_is_modeled(line_type) {
        return None;
    }

    let width = if line.width > 0 {
        hwp_units_to_px_option(line.width as u32)?
    } else {
        LengthPx(96.0 / 254.0)
    };
    let style = match line_type {
        2 | 4..=6 => BorderStyle::Dashed,
        3 | 7 => BorderStyle::Dotted,
        8..=11 | 13 => BorderStyle::Double,
        _ => BorderStyle::Solid,
    };

    Some(Border {
        width,
        style,
        color: color_ref_to_color_option(line.color),
    })
}

fn map_shape_background_color(drawing: &rhwp::model::shape::DrawingObjAttr) -> Option<Color> {
    let solid = drawing.fill.solid.as_ref()?;
    (drawing.fill.fill_type == RhwpFillType::Solid && solid.pattern_type <= 0)
        .then(|| color_ref_to_color_option(solid.background_color))
        .flatten()
}

fn shape_unmodeled_presentation_details(drawing: &rhwp::model::shape::DrawingObjAttr) -> String {
    let mut details = Vec::new();
    let line_type = (drawing.border_line.attr & 0x3f) as u8;
    if line_type != 0 && !picture_border_line_type_is_modeled(line_type) {
        details.push(format!("border line type={line_type}"));
    }
    match drawing.fill.fill_type {
        RhwpFillType::None => {}
        RhwpFillType::Solid => match drawing.fill.solid.as_ref() {
            Some(solid) if solid.pattern_type <= 0 => {}
            Some(solid) => details.push(format!("pattern fill type={}", solid.pattern_type)),
            None => details.push("solid fill data unavailable".to_string()),
        },
        RhwpFillType::Image => details.push("image fill".to_string()),
        RhwpFillType::Gradient => details.push("gradient fill".to_string()),
    }
    if drawing.shadow_type != 0 {
        details.push(format!("shadow_type={}", drawing.shadow_type));
    }
    if details.is_empty() {
        "no additional drawing effects".to_string()
    } else {
        format!("unmodeled {}", details.join(", "))
    }
}

fn picture_border_line_type_is_modeled(line_type: u8) -> bool {
    matches!(line_type, 1..=11 | 13)
}

fn table_border_line_type_is_approximated(line_type: RhwpBorderLineType) -> bool {
    matches!(
        line_type,
        RhwpBorderLineType::Wave
            | RhwpBorderLineType::DoubleWave
            | RhwpBorderLineType::Thick3D
            | RhwpBorderLineType::Thick3DReverse
            | RhwpBorderLineType::Thin3D
            | RhwpBorderLineType::Thin3DReverse
    )
}

fn map_border_line_type(line_type: RhwpBorderLineType) -> BorderStyle {
    match line_type {
        RhwpBorderLineType::Dash
        | RhwpBorderLineType::LongDash
        | RhwpBorderLineType::DashDot
        | RhwpBorderLineType::DashDotDot => BorderStyle::Dashed,
        RhwpBorderLineType::Dot | RhwpBorderLineType::Circle => BorderStyle::Dotted,
        RhwpBorderLineType::Double
        | RhwpBorderLineType::ThinThickDouble
        | RhwpBorderLineType::ThickThinDouble
        | RhwpBorderLineType::ThinThickThinTriple
        | RhwpBorderLineType::DoubleWave => BorderStyle::Double,
        // Solid, Wave, and the 3D line types have no direct CSS equivalent and
        // fall back to a solid line.
        _ => BorderStyle::Solid,
    }
}

/// Convert an HWP border width index to px. The 0-7 thresholds match rhwp's own
/// `css_border_width_to_hwp` table; 8-15 use the standard HWP preset widths.
/// 1mm ≈ 3.7795px at 96dpi.
fn border_width_index_to_px(index: u8) -> LengthPx {
    const WIDTHS_MM: [f32; 16] = [
        0.1, 0.12, 0.15, 0.2, 0.25, 0.3, 0.4, 0.5, 0.6, 0.7, 1.0, 1.5, 2.0, 3.0, 4.0, 5.0,
    ];
    let mm = WIDTHS_MM[(index as usize).min(WIDTHS_MM.len() - 1)];
    LengthPx(mm * 96.0 / 25.4)
}

fn i32_hwp_units_to_pt_option(value: i32) -> Option<LengthPt> {
    if value == 0 {
        None
    } else {
        Some(LengthPt(value as f32 / 100.0))
    }
}

fn i16_hwp_units_to_pt_option(value: i16) -> Option<LengthPt> {
    (value > 0).then(|| LengthPt(value as f32 / 100.0))
}

fn paragraph_layout_flag(para_shape: &RhwpParaShape, attr1_bit: u32, attr2_bit: u32) -> bool {
    para_shape.attr1 & (1 << attr1_bit) != 0 || para_shape.attr2 & (1 << attr2_bit) != 0
}

fn color_ref_to_color_option(color_ref: u32) -> Option<Color> {
    // COLORREF uses only the low 24 bits for RGB. A non-zero high byte is used
    // by HWP/rHWP for CLR_INVALID/default/transparent sentinels.
    if color_ref >> 24 != 0 {
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

fn map_text_decoration_style(shape: u8) -> TextDecorationStyle {
    match shape {
        1 | 3..=5 => TextDecorationStyle::Dashed,
        2 | 6 => TextDecorationStyle::Dotted,
        7..=10 => TextDecorationStyle::Double,
        11 | 12 => TextDecorationStyle::Wavy,
        _ => TextDecorationStyle::Solid,
    }
}

fn decoration_shape_is_approximated(shape: u8) -> bool {
    matches!(shape, 3..=6 | 8..=10 | 12..=u8::MAX)
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

fn map_picture_crop(picture: &Picture) -> Option<ImageCrop> {
    let crop = picture.crop;
    (crop.left >= 0 && crop.top >= 0 && crop.right > crop.left && crop.bottom > crop.top).then_some(
        ImageCrop {
            left: LengthPx(crop.left as f32 / 75.0),
            top: LengthPx(crop.top as f32 / 75.0),
            right: LengthPx(crop.right as f32 / 75.0),
            bottom: LengthPx(crop.bottom as f32 / 75.0),
            source_width: None,
            source_height: None,
        },
    )
}

fn map_picture_placement(picture: &Picture) -> Option<ImagePlacement> {
    let common = &picture.common;
    let has_layout = !common.treat_as_char
        || common.horizontal_offset != 0
        || common.vertical_offset != 0
        || common.text_wrap != rhwp::model::shape::TextWrap::Square
        || common.vert_rel_to != rhwp::model::shape::VertRelTo::Paper
        || common.horz_rel_to != rhwp::model::shape::HorzRelTo::Paper
        || common.vert_align != rhwp::model::shape::VertAlign::Top
        || common.horz_align != rhwp::model::shape::HorzAlign::Left;
    has_layout.then_some(ImagePlacement {
        treat_as_character: common.treat_as_char,
        text_wrap: match common.text_wrap {
            rhwp::model::shape::TextWrap::Square => ImageTextWrap::Square,
            rhwp::model::shape::TextWrap::Tight => ImageTextWrap::Tight,
            rhwp::model::shape::TextWrap::Through => ImageTextWrap::Through,
            rhwp::model::shape::TextWrap::TopAndBottom => ImageTextWrap::TopAndBottom,
            rhwp::model::shape::TextWrap::BehindText => ImageTextWrap::BehindText,
            rhwp::model::shape::TextWrap::InFrontOfText => ImageTextWrap::InFrontOfText,
        },
        vertical_relative_to: match common.vert_rel_to {
            rhwp::model::shape::VertRelTo::Paper => VerticalRelativeTo::Paper,
            rhwp::model::shape::VertRelTo::Page => VerticalRelativeTo::Page,
            rhwp::model::shape::VertRelTo::Para => VerticalRelativeTo::Paragraph,
        },
        vertical_alignment: match common.vert_align {
            rhwp::model::shape::VertAlign::Top => VerticalObjectAlignment::Top,
            rhwp::model::shape::VertAlign::Center => VerticalObjectAlignment::Center,
            rhwp::model::shape::VertAlign::Bottom => VerticalObjectAlignment::Bottom,
            rhwp::model::shape::VertAlign::Inside => VerticalObjectAlignment::Inside,
            rhwp::model::shape::VertAlign::Outside => VerticalObjectAlignment::Outside,
        },
        vertical_offset: LengthPx(common.vertical_offset as f32 / 75.0),
        horizontal_relative_to: match common.horz_rel_to {
            rhwp::model::shape::HorzRelTo::Paper => HorizontalRelativeTo::Paper,
            rhwp::model::shape::HorzRelTo::Page => HorizontalRelativeTo::Page,
            rhwp::model::shape::HorzRelTo::Column => HorizontalRelativeTo::Column,
            rhwp::model::shape::HorzRelTo::Para => HorizontalRelativeTo::Paragraph,
        },
        horizontal_alignment: match common.horz_align {
            rhwp::model::shape::HorzAlign::Left => HorizontalObjectAlignment::Left,
            rhwp::model::shape::HorzAlign::Center => HorizontalObjectAlignment::Center,
            rhwp::model::shape::HorzAlign::Right => HorizontalObjectAlignment::Right,
            rhwp::model::shape::HorzAlign::Inside => HorizontalObjectAlignment::Inside,
            rhwp::model::shape::HorzAlign::Outside => HorizontalObjectAlignment::Outside,
        },
        horizontal_offset: LengthPx(common.horizontal_offset as f32 / 75.0),
    })
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

fn normalize_bullet_char(ch: char) -> Option<char> {
    if ch == '\u{FFFF}' || ch.is_control() {
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
        CommonObjAttr as RhwpCommonObjAttr, DrawingObjAttr as RhwpDrawingObjAttr,
        GroupShape as RhwpGroupShape, RectangleShape as RhwpRectangleShape,
        ShapeComponentAttr as RhwpShapeComponentAttr, TextBox as RhwpTextBox,
    };
    use rhwp::model::style::{
        Alignment as RhwpAlignment, BorderFill as RhwpBorderFill, Bullet as RhwpBullet,
        CharShape as RhwpCharShape, Fill, FillType, Font, HeadType as RhwpHeadType,
        ParaShape as RhwpParaShape, ShapeBorderLine as RhwpShapeBorderLine, SolidFill,
        Style as RhwpStyle, UnderlineType as RhwpUnderlineType,
    };
    use rhwp::model::table::{
        Cell as RhwpCell, Table as RhwpTable, VerticalAlign as RhwpVerticalAlign,
    };

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
    fn preserves_table_size_and_outer_margins_and_warns_for_remaining_layout() {
        let table = RhwpTable {
            row_count: 1,
            col_count: 1,
            cell_spacing: 75,
            row_sizes: vec![1500],
            zones: vec![rhwp::model::table::TableZone {
                start_col: 0,
                start_row: 0,
                end_col: 0,
                end_row: 0,
                border_fill_id: 2,
            }],
            page_break: rhwp::model::table::TablePageBreak::RowBreak,
            repeat_header: true,
            outer_margin_left: 100,
            outer_margin_right: 200,
            outer_margin_top: 300,
            outer_margin_bottom: 400,
            common: RhwpCommonObjAttr {
                width: 7500,
                height: 3000,
                horizontal_offset: 500,
                vertical_offset: 600,
                z_order: 2,
                ..Default::default()
            },
            cells: vec![RhwpCell {
                row: 0,
                col: 0,
                paragraphs: vec![RhwpParagraph {
                    text: "cell".to_string(),
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

        let Block::Table(table) = &bridged.sections[0].blocks[0] else {
            panic!("expected table block");
        };
        assert_eq!(table.style.width, Some(LengthPx(100.0)));
        assert_eq!(table.style.height, Some(LengthPx(40.0)));
        assert_eq!(table.style.margin_left, Some(LengthPx(100.0 / 75.0)));
        assert_eq!(table.style.margin_right, Some(LengthPx(200.0 / 75.0)));
        assert_eq!(table.style.margin_top, Some(LengthPx(4.0)));
        assert_eq!(table.style.margin_bottom, Some(LengthPx(400.0 / 75.0)));
        assert_eq!(table.rows[0].height, Some(LengthPx(20.0)));
        assert_eq!(table.style.cell_spacing, Some(LengthPx(1.0)));
        assert!(table.style.repeat_header);
        assert_eq!(table.style.page_break, Some(TablePageBreak::Row));

        let warning = bridged
            .warnings
            .iter()
            .find(|warning| warning.message.contains("table layout properties"))
            .expect("table layout warning");
        assert!(
            warning.message.contains("border_fill_zones=1"),
            "missing border fill zone warning: {}",
            warning.message
        );
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
    fn maps_table_cell_header_and_vertical_align() {
        let header_cell = RhwpCell {
            row: 0,
            col: 0,
            row_span: 1,
            col_span: 1,
            is_header: true,
            vertical_align: RhwpVerticalAlign::Center,
            width: 7500,
            height: 1500,
            padding: rhwp::model::Padding {
                left: 150,
                right: 150,
                top: 75,
                bottom: 75,
            },
            apply_inner_margin: true,
            paragraphs: vec![RhwpParagraph {
                text: "h".to_string(),
                ..Default::default()
            }],
            ..Default::default()
        };
        let plain_cell = RhwpCell {
            row: 0,
            col: 1,
            row_span: 1,
            col_span: 1,
            paragraphs: vec![RhwpParagraph {
                text: "p".to_string(),
                ..Default::default()
            }],
            ..Default::default()
        };
        let table = RhwpTable {
            row_count: 1,
            col_count: 2,
            padding: rhwp::model::Padding {
                left: 300,
                right: 300,
                top: 225,
                bottom: 225,
            },
            cells: vec![header_cell, plain_cell],
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
                let header = &table.rows[0].cells[0];
                assert!(header.is_header);
                assert_eq!(
                    header.style.vertical_align,
                    Some(crate::ir::VerticalAlign::Middle)
                );
                assert_eq!(header.style.width, Some(crate::ir::LengthPx(100.0)));
                assert_eq!(header.style.height, Some(crate::ir::LengthPx(20.0)));
                assert_eq!(header.style.padding_left, Some(crate::ir::LengthPx(2.0)));
                assert_eq!(header.style.padding_top, Some(crate::ir::LengthPx(1.0)));

                let plain = &table.rows[0].cells[1];
                assert!(!plain.is_header);
                assert_eq!(plain.style.vertical_align, None);
                assert_eq!(plain.style.width, None);
                assert_eq!(plain.style.padding_left, Some(crate::ir::LengthPx(4.0)));
                assert_eq!(plain.style.padding_top, Some(crate::ir::LengthPx(3.0)));
            }
            other => panic!("expected table block, got {other:?}"),
        }
    }

    #[test]
    fn warns_and_omits_negative_table_padding_sides() {
        let document = RhwpDocument {
            sections: vec![RhwpSection {
                paragraphs: vec![RhwpParagraph {
                    controls: vec![Control::Table(Box::new(RhwpTable {
                        row_count: 1,
                        col_count: 1,
                        padding: rhwp::model::Padding {
                            left: -75,
                            right: 150,
                            top: -1,
                            bottom: 225,
                        },
                        cells: vec![RhwpCell {
                            row: 0,
                            col: 0,
                            paragraphs: vec![RhwpParagraph {
                                text: "cell".to_string(),
                                ..Default::default()
                            }],
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
                let style = &table.rows[0].cells[0].style;
                assert_eq!(style.padding_left, None);
                assert_eq!(style.padding_right, Some(LengthPx(2.0)));
                assert_eq!(style.padding_top, None);
                assert_eq!(style.padding_bottom, Some(LengthPx(3.0)));
            }
            other => panic!("expected table block, got {other:?}"),
        }
        assert!(bridged.warnings.iter().any(|warning| {
            warning.message.contains("table-default padding")
                && warning.message.contains("negative HWPUNIT")
        }));
    }

    #[test]
    fn maps_table_cell_borders_from_border_fill() {
        let border = |line_type, width, color| RhwpBorderLine {
            line_type,
            width,
            color,
        };
        let document = RhwpDocument {
            doc_info: DocInfo {
                border_fills: vec![RhwpBorderFill {
                    // rhwp order: left, right, top, bottom
                    borders: [
                        border(RhwpBorderLineType::Solid, 7, 0x00112233),
                        border(RhwpBorderLineType::Dash, 1, 0),
                        border(RhwpBorderLineType::None, 0, 0),
                        border(RhwpBorderLineType::Dot, 1, 0),
                    ],
                    ..Default::default()
                }],
                ..Default::default()
            },
            sections: vec![RhwpSection {
                paragraphs: vec![RhwpParagraph {
                    controls: vec![Control::Table(Box::new(RhwpTable {
                        row_count: 1,
                        col_count: 1,
                        cells: vec![RhwpCell {
                            row: 0,
                            col: 0,
                            border_fill_id: 1,
                            paragraphs: vec![RhwpParagraph {
                                text: "c".to_string(),
                                ..Default::default()
                            }],
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
                let style = &table.rows[0].cells[0].style;
                let left = style.border_left.as_ref().expect("left border");
                assert_eq!(left.style, BorderStyle::Solid);
                assert_eq!(
                    left.color,
                    Some(Color {
                        r: 0x33,
                        g: 0x22,
                        b: 0x11,
                        a: 255,
                    })
                );
                assert_eq!(
                    style.border_right.as_ref().expect("right border").style,
                    BorderStyle::Dashed
                );
                assert!(style.border_top.is_none());
                assert_eq!(
                    style.border_bottom.as_ref().expect("bottom border").style,
                    BorderStyle::Dotted
                );
            }
            other => panic!("expected table block, got {other:?}"),
        }
    }

    #[test]
    fn warns_when_table_border_line_type_is_approximated() {
        let border = |line_type| RhwpBorderLine {
            line_type,
            width: 1,
            color: 0,
        };
        let document = RhwpDocument {
            doc_info: DocInfo {
                border_fills: vec![RhwpBorderFill {
                    borders: [
                        border(RhwpBorderLineType::Wave),
                        border(RhwpBorderLineType::DoubleWave),
                        border(RhwpBorderLineType::Thick3D),
                        border(RhwpBorderLineType::None),
                    ],
                    ..Default::default()
                }],
                ..Default::default()
            },
            sections: vec![RhwpSection {
                paragraphs: vec![RhwpParagraph {
                    controls: vec![Control::Table(Box::new(RhwpTable {
                        row_count: 1,
                        col_count: 1,
                        cells: vec![RhwpCell {
                            row: 0,
                            col: 0,
                            border_fill_id: 1,
                            paragraphs: vec![RhwpParagraph {
                                text: "c".to_string(),
                                ..Default::default()
                            }],
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
                let style = &table.rows[0].cells[0].style;
                assert_eq!(
                    style.border_left.as_ref().expect("left border").style,
                    BorderStyle::Solid
                );
                assert_eq!(
                    style.border_right.as_ref().expect("right border").style,
                    BorderStyle::Double
                );
                assert_eq!(
                    style.border_top.as_ref().expect("top border").style,
                    BorderStyle::Solid
                );
                assert!(style.border_bottom.is_none());
            }
            other => panic!("expected table block, got {other:?}"),
        }

        assert!(bridged.warnings.iter().any(|warning| {
            warning.message.contains("border line type Wave")
                && warning.message.contains("approximated")
        }));
        assert!(bridged.warnings.iter().any(|warning| {
            warning.message.contains("border line type DoubleWave")
                && warning.message.contains("approximated")
        }));
        assert!(bridged.warnings.iter().any(|warning| {
            warning.message.contains("border line type Thick3D")
                && warning.message.contains("approximated")
        }));
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
                width: 3600,
                height: 1800,
                horizontal_offset: 300,
                vertical_offset: 400,
                ..Default::default()
            },
            drawing: RhwpDrawingObjAttr {
                shape_attr: RhwpShapeComponentAttr {
                    rotation_angle: 90,
                    horz_flip: true,
                    vert_flip: true,
                    ..Default::default()
                },
                border_line: RhwpShapeBorderLine {
                    color: 0x00332211,
                    width: 150,
                    attr: 3,
                    ..Default::default()
                },
                fill: Fill {
                    fill_type: FillType::Solid,
                    solid: Some(SolidFill {
                        background_color: 0x00665544,
                        ..Default::default()
                    }),
                    ..Default::default()
                },
                text_box: Some(RhwpTextBox {
                    vertical_align: RhwpVerticalAlign::Center,
                    margin_left: 75,
                    margin_right: 150,
                    margin_top: 225,
                    margin_bottom: 300,
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
                assert_eq!(shape.width, Some(LengthPx(48.0)));
                assert_eq!(shape.height, Some(LengthPx(24.0)));
                assert_eq!(shape.offset_x, Some(LengthPx(4.0)));
                assert_eq!(shape.offset_y, Some(LengthPx(400.0 / 75.0)));
                assert_eq!(shape.rotation_degrees, Some(90.0));
                assert_eq!(shape.flip_horizontal, Some(true));
                assert_eq!(shape.flip_vertical, Some(true));
                assert_eq!(shape.text_vertical_align, Some(VerticalAlign::Middle));
                assert_eq!(shape.padding_top, Some(LengthPx(3.0)));
                assert_eq!(shape.padding_right, Some(LengthPx(2.0)));
                assert_eq!(shape.padding_bottom, Some(LengthPx(4.0)));
                assert_eq!(shape.padding_left, Some(LengthPx(1.0)));
                assert_eq!(
                    shape.background_color,
                    Some(Color {
                        r: 0x44,
                        g: 0x55,
                        b: 0x66,
                        a: 255,
                    })
                );
                assert_eq!(
                    shape.border,
                    Some(Border {
                        width: LengthPx(2.0),
                        style: BorderStyle::Dotted,
                        color: Some(Color {
                            r: 0x11,
                            g: 0x22,
                            b: 0x33,
                            a: 255,
                        }),
                    })
                );
            }
            other => panic!("expected shape block, got {other:?}"),
        }
        assert!(
            bridged
                .warnings
                .iter()
                .any(|warning| { warning.message.contains("shape text box paragraphs") })
        );
        assert!(bridged.warnings.iter().any(|warning| {
            warning.message.contains("shape Rectangle remains")
                && warning.message.contains("semantic placeholder")
        }));
    }

    #[test]
    fn preserves_equation_presentation_metadata() {
        let equation = RhwpEquation {
            common: RhwpCommonObjAttr {
                width: 7500,
                height: 1500,
                horizontal_offset: 300,
                vertical_offset: 400,
                ..Default::default()
            },
            script: "x over y".to_string(),
            font_size: 1200,
            color: 0x00112233,
            baseline: -10,
            font_name: "HancomEQN".to_string(),
            version_info: "60".to_string(),
            ..Default::default()
        };
        let document = RhwpDocument {
            sections: vec![RhwpSection {
                paragraphs: vec![RhwpParagraph {
                    controls: vec![Control::Equation(Box::new(equation))],
                    ..Default::default()
                }],
                ..Default::default()
            }],
            ..Default::default()
        };

        let bridged = BridgeContext::new(&document).into_document();

        assert!(matches!(
            &bridged.sections[0].blocks[0],
            Block::Equation(equation)
                if equation.content.as_deref() == Some("x over y")
                    && equation.fallback_text.as_deref() == Some("x over y")
                    && equation.font_size_pt == Some(LengthPt(12.0))
                    && equation.color == Some(Color { r: 0x33, g: 0x22, b: 0x11, a: 255 })
                    && equation.baseline_pt == Some(LengthPt(-0.1))
                    && equation.font_family.as_deref() == Some("HancomEQN")
                    && equation.version.as_deref() == Some("60")
                    && equation.width == Some(LengthPx(100.0))
                    && equation.height == Some(LengthPx(20.0))
                    && equation.offset_x == Some(LengthPx(4.0))
                    && equation.offset_y == Some(LengthPx(400.0 / 75.0))
        ));
        assert!(
            !bridged
                .warnings
                .iter()
                .any(|warning| warning.message.contains("equation presentation metadata"))
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
    fn maps_picture_border_and_grayscale() {
        let picture = Picture {
            image_attr: ImageAttr {
                bin_data_id: 7,
                effect: RhwpImageEffect::GrayScale,
                ..Default::default()
            },
            common: rhwp::model::shape::CommonObjAttr {
                width: 7500,
                height: 3750,
                ..Default::default()
            },
            border_width: 75,
            border_color: 0x00112233,
            border_attr: rhwp::model::style::ShapeBorderLine {
                attr: 2,
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
                assert!(image.grayscale);
                let border = image.border.as_ref().expect("image border");
                assert_eq!(border.width, LengthPx(1.0));
                assert_eq!(border.style, BorderStyle::Dashed);
                assert_eq!(
                    border.color,
                    Some(Color {
                        r: 0x33,
                        g: 0x22,
                        b: 0x11,
                        a: 255,
                    })
                );
            }
            other => panic!("expected image block, got {other:?}"),
        }
    }

    #[test]
    fn maps_picture_border_line_type_and_default_width() {
        let dotted = Picture {
            border_attr: rhwp::model::style::ShapeBorderLine {
                attr: 3,
                ..Default::default()
            },
            ..Default::default()
        };
        let disabled = Picture {
            border_width: 75,
            border_attr: rhwp::model::style::ShapeBorderLine {
                attr: 0,
                ..Default::default()
            },
            ..Default::default()
        };

        let border = map_image_border(&dotted).expect("active picture border");
        assert_eq!(border.style, BorderStyle::Dotted);
        assert!((border.width.0 - (96.0 / 254.0)).abs() < f32::EPSILON);
        assert_eq!(map_image_border(&disabled), None);
    }

    #[test]
    fn warns_when_picture_border_line_type_is_approximated() {
        let document = RhwpDocument {
            sections: vec![RhwpSection {
                paragraphs: vec![RhwpParagraph {
                    controls: vec![Control::Picture(Box::new(Picture {
                        common: RhwpCommonObjAttr {
                            width: 7500,
                            height: 7500,
                            ..Default::default()
                        },
                        image_attr: ImageAttr {
                            bin_data_id: 7,
                            ..Default::default()
                        },
                        border_width: 75,
                        border_attr: rhwp::model::style::ShapeBorderLine {
                            attr: 12,
                            ..Default::default()
                        },
                        ..Default::default()
                    }))],
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

        assert!(bridged.warnings.iter().any(|warning| {
            warning.message.contains("picture border line type 12")
                && warning.message.contains("solid border")
        }));
    }

    #[test]
    fn warns_when_black_white_picture_effect_is_approximated() {
        let picture = Picture {
            image_attr: ImageAttr {
                bin_data_id: 7,
                effect: RhwpImageEffect::BlackWhite,
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

        assert!(matches!(
            &bridged.sections[0].blocks[0],
            Block::Image(image) if image.grayscale
        ));
        assert!(bridged.warnings.iter().any(|warning| {
            warning.message.contains("BlackWhite effect")
                && warning.message.contains("grayscale approximation")
        }));
    }

    #[test]
    fn warns_when_picture_bin_data_is_missing() {
        let picture = Picture {
            image_attr: ImageAttr {
                bin_data_id: 7,
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
            ..Default::default()
        };

        let bridged = BridgeContext::new(&document).into_document();

        assert!(matches!(
            &bridged.sections[0].blocks[0],
            Block::Unknown(unknown)
                if unknown.kind == "picture"
                    && unknown.fallback_text.as_deref() == Some("[image]")
        ));
        assert!(bridged.warnings.iter().any(|warning| {
            warning
                .message
                .contains("picture referenced missing bin data 7")
        }));
    }

    #[test]
    fn preserves_picture_caption_field_fallback_text() {
        let picture = Picture {
            image_attr: ImageAttr {
                bin_data_id: 7,
                ..Default::default()
            },
            caption: Some(RhwpCaption {
                direction: RhwpCaptionDirection::Left,
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
                assert_eq!(image.caption_placement, Some(CaptionPlacement::Left));
            }
            other => panic!("expected image block, got {other:?}"),
        }
    }

    #[test]
    fn preserves_picture_rotation_and_flip_and_warns_for_remaining_effects() {
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
                left: -10,
                right: 20,
                top: 30,
                bottom: 40,
            },
            shape_attr: rhwp::model::shape::ShapeComponentAttr {
                horz_flip: true,
                rotation_angle: 90,
                render_b: 0.25,
                render_c: -0.25,
                ..Default::default()
            },
            common: rhwp::model::shape::CommonObjAttr {
                text_wrap: rhwp::model::shape::TextWrap::TopAndBottom,
                vert_rel_to: rhwp::model::shape::VertRelTo::Page,
                vert_align: rhwp::model::shape::VertAlign::Center,
                vertical_offset: 120,
                horz_rel_to: rhwp::model::shape::HorzRelTo::Column,
                horz_align: rhwp::model::shape::HorzAlign::Right,
                horizontal_offset: 240,
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

        let Block::Image(image) = &bridged.sections[0].blocks[0] else {
            panic!("expected image block");
        };
        assert_eq!(image.rotation_degrees, Some(90.0));
        assert_eq!(image.flip_horizontal, Some(true));
        assert_eq!(image.flip_vertical, None);
        assert_eq!(image.brightness, Some(10));
        assert_eq!(image.contrast, Some(-5));
        assert_eq!(image.effect, Some(IrImageEffect::Grayscale));
        assert_eq!(
            image.placement,
            Some(ImagePlacement {
                treat_as_character: false,
                text_wrap: ImageTextWrap::TopAndBottom,
                vertical_relative_to: VerticalRelativeTo::Page,
                vertical_alignment: VerticalObjectAlignment::Center,
                vertical_offset: LengthPx(120.0 / 75.0),
                horizontal_relative_to: HorizontalRelativeTo::Column,
                horizontal_alignment: HorizontalObjectAlignment::Right,
                horizontal_offset: LengthPx(240.0 / 75.0),
            })
        );
        assert_eq!(image.padding_top, Some(LengthPx(30.0 / 75.0)));
        assert_eq!(image.padding_right, Some(LengthPx(20.0 / 75.0)));
        assert_eq!(image.padding_bottom, Some(LengthPx(40.0 / 75.0)));
        assert_eq!(image.padding_left, None);
        assert_eq!(
            image.crop,
            Some(ImageCrop {
                left: LengthPx(1.0 / 75.0),
                top: LengthPx(2.0 / 75.0),
                right: LengthPx(3.0 / 75.0),
                bottom: LengthPx(4.0 / 75.0),
                source_width: None,
                source_height: None,
            })
        );
        assert!(bridged.warnings.iter().any(|warning| {
            warning.message.contains("picture crop")
                && warning.message.contains("preserved in Image IR")
        }));
        assert!(bridged.warnings.iter().any(|warning| {
            warning.message.contains("brightness:10,contrast:-5")
                && warning.message.contains("preserved in Image IR")
        }));
        assert!(bridged.warnings.iter().any(|warning| {
            warning.message.contains("picture layout")
                && warning.message.contains("preserved in Image IR")
                && warning.message.contains("vertical:Page/Center/120")
                && warning.message.contains("horizontal:Column/Right/240")
        }));
        assert!(bridged.warnings.iter().any(|warning| {
            warning.message.contains("picture visual properties")
                && warning
                    .message
                    .contains("affine_shear_or_rotation=0.25/-0.25")
                && warning.message.contains("border_opacity=128")
                && warning.message.contains("negative_padding=-10/20/30/40")
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
                    superscript: true,
                    emphasis_dot: 1,
                    emboss: true,
                    outline_type: 1,
                    shadow_type: 1,
                    base_size: 1200,
                    text_color: 0x00010203,
                    shade_color: 0x00040506,
                    underline_color: 0x00112233,
                    strike_color: 0x00445566,
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
                        assert!(run.style.superscript);
                        assert!(!run.style.subscript);
                        assert!(run.style.emphasis_dot);
                        assert!(run.style.emboss);
                        assert!(!run.style.engrave);
                        assert!(run.style.outline);
                        assert!(run.style.shadow);
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
                        assert_eq!(
                            run.style.underline_color,
                            Some(Color {
                                r: 0x33,
                                g: 0x22,
                                b: 0x11,
                                a: 255,
                            })
                        );
                        assert_eq!(
                            run.style.strike_color,
                            Some(Color {
                                r: 0x66,
                                g: 0x55,
                                b: 0x44,
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
    fn warns_when_text_style_details_are_approximated_or_omitted() {
        let document = RhwpDocument {
            doc_info: DocInfo {
                char_shapes: vec![RhwpCharShape {
                    ratios: [120, 100, 100, 100, 100, 100, 100],
                    spacings: [5, 0, 0, 0, 0, 0, 0],
                    relative_sizes: [110, 100, 100, 100, 100, 100, 100],
                    char_offsets: [10, 0, 0, 0, 0, 0, 0],
                    base_size: -100,
                    underline_type: RhwpUnderlineType::Top,
                    underline_shape: 2,
                    strikethrough: true,
                    strike_shape: 3,
                    emphasis_dot: 2,
                    outline_type: 2,
                    shadow_type: 1,
                    kerning: true,
                    ..Default::default()
                }],
                ..Default::default()
            },
            sections: vec![RhwpSection {
                paragraphs: vec![RhwpParagraph {
                    text: "styled".to_string(),
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
        let run = match &bridged.sections[0].blocks[0] {
            Block::Paragraph(paragraph) => match &paragraph.inlines[0] {
                Inline::Text(run) => run,
                other => panic!("expected text run, got {other:?}"),
            },
            other => panic!("expected paragraph block, got {other:?}"),
        };

        assert_eq!(run.style.font_size_pt, None);
        assert!(run.style.kerning);
        assert!(run.style.underline_above);
        assert_eq!(run.style.underline_style, Some(TextDecorationStyle::Dotted));
        assert_eq!(run.style.strike_style, Some(TextDecorationStyle::Dashed));
        for expected in [
            "strike shape 3",
            "different simultaneous underline and strike",
            "emphasis mark type 2",
            "outline type 2",
            "shadow type",
            "invalid font size -100",
        ] {
            assert!(
                bridged
                    .warnings
                    .iter()
                    .any(|warning| warning.message.contains(expected)),
                "missing warning fragment {expected:?}: {:#?}",
                bridged.warnings
            );
        }
    }

    #[test]
    fn maps_uniform_text_metrics_and_effective_font_size() {
        let document = RhwpDocument {
            doc_info: DocInfo {
                char_shapes: vec![RhwpCharShape {
                    ratios: [95; 7],
                    spacings: [-5; 7],
                    relative_sizes: [80; 7],
                    char_offsets: [10; 7],
                    base_size: 1000,
                    kerning: true,
                    ..Default::default()
                }],
                ..Default::default()
            },
            sections: vec![RhwpSection {
                paragraphs: vec![RhwpParagraph {
                    text: "metrics".to_string(),
                    char_offsets: vec![0, 1, 2, 3, 4, 5, 6],
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
        let run = match &bridged.sections[0].blocks[0] {
            Block::Paragraph(paragraph) => match &paragraph.inlines[0] {
                Inline::Text(run) => run,
                other => panic!("expected text run, got {other:?}"),
            },
            other => panic!("expected paragraph block, got {other:?}"),
        };

        assert_eq!(run.style.font_width_percent, Some(Percent(95.0)));
        assert_eq!(run.style.letter_spacing_percent, Some(Percent(-5.0)));
        assert_eq!(run.style.relative_size_percent, Some(Percent(80.0)));
        assert_eq!(run.style.vertical_offset_percent, Some(Percent(10.0)));
        assert_eq!(run.style.font_size_pt, Some(LengthPt(8.0)));
        assert!(run.style.kerning);
        assert!(!bridged.warnings.iter().any(|warning| {
            warning.message.contains("script-specific") || warning.message.contains("kerning")
        }));
    }

    #[test]
    fn splits_mixed_script_text_and_preserves_script_specific_styles() {
        let document = RhwpDocument {
            doc_info: DocInfo {
                font_faces: vec![
                    vec![Font {
                        name: "Korean Font".to_string(),
                        ..Default::default()
                    }],
                    vec![Font {
                        name: "Latin Font".to_string(),
                        ..Default::default()
                    }],
                ],
                char_shapes: vec![RhwpCharShape {
                    font_ids: [0; 7],
                    ratios: [90, 110, 100, 100, 100, 100, 100],
                    spacings: [-5, 5, 0, 0, 0, 0, 0],
                    relative_sizes: [80, 120, 100, 100, 100, 100, 100],
                    char_offsets: [10, -10, 0, 0, 0, 0, 0],
                    base_size: 1000,
                    ..Default::default()
                }],
                ..Default::default()
            },
            sections: vec![RhwpSection {
                paragraphs: vec![RhwpParagraph {
                    text: "한글 English".to_string(),
                    char_offsets: (0..10).collect(),
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
        let inlines = match &bridged.sections[0].blocks[0] {
            Block::Paragraph(paragraph) => &paragraph.inlines,
            other => panic!("expected paragraph block, got {other:?}"),
        };

        assert_eq!(inlines.len(), 2);
        let korean = match &inlines[0] {
            Inline::Text(run) => run,
            other => panic!("expected Korean text run, got {other:?}"),
        };
        let latin = match &inlines[1] {
            Inline::Text(run) => run,
            other => panic!("expected Latin text run, got {other:?}"),
        };
        assert_eq!(korean.text, "한글 ");
        assert_eq!(korean.style.font_family.as_deref(), Some("Korean Font"));
        assert_eq!(korean.style.font_width_percent, Some(Percent(90.0)));
        assert_eq!(korean.style.letter_spacing_percent, Some(Percent(-5.0)));
        assert_eq!(korean.style.font_size_pt, Some(LengthPt(8.0)));
        assert_eq!(korean.style.vertical_offset_percent, Some(Percent(10.0)));

        assert_eq!(latin.text, "English");
        assert_eq!(latin.style.font_family.as_deref(), Some("Latin Font"));
        assert_eq!(latin.style.font_width_percent, Some(Percent(110.0)));
        assert_eq!(latin.style.letter_spacing_percent, Some(Percent(5.0)));
        assert_eq!(latin.style.font_size_pt, Some(LengthPt(12.0)));
        assert_eq!(latin.style.vertical_offset_percent, Some(Percent(-10.0)));
        assert!(
            !bridged
                .warnings
                .iter()
                .any(|warning| { warning.message.contains("script-specific") })
        );
    }

    #[test]
    fn warns_for_missing_active_script_font_and_uses_fallback() {
        let document = RhwpDocument {
            doc_info: DocInfo {
                font_faces: vec![
                    vec![Font {
                        name: "Korean Font".to_string(),
                        ..Default::default()
                    }],
                    Vec::new(),
                    vec![Font {
                        name: "Hanja Font".to_string(),
                        ..Default::default()
                    }],
                ],
                char_shapes: vec![RhwpCharShape {
                    font_ids: [0, 2, 0, 0, 0, 0, 0],
                    ..Default::default()
                }],
                ..Default::default()
            },
            sections: vec![RhwpSection {
                paragraphs: vec![RhwpParagraph {
                    text: "fonts".to_string(),
                    char_offsets: vec![0, 1, 2, 3, 4],
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
        let run = match &bridged.sections[0].blocks[0] {
            Block::Paragraph(paragraph) => match &paragraph.inlines[0] {
                Inline::Text(run) => run,
                other => panic!("expected text run, got {other:?}"),
            },
            other => panic!("expected paragraph block, got {other:?}"),
        };

        assert_eq!(run.style.font_family.as_deref(), Some("Korean Font"));
        assert!(bridged.warnings.iter().any(|warning| {
            warning
                .message
                .contains("missing font id 2 in font face group 1")
        }));
        assert!(!bridged.warnings.iter().any(|warning| {
            warning
                .message
                .contains("multiple script-specific font families")
        }));
    }

    #[test]
    fn uses_active_script_font_when_another_script_ref_is_missing() {
        let document = RhwpDocument {
            doc_info: DocInfo {
                font_faces: vec![
                    Vec::new(),
                    vec![Font {
                        name: "Fallback Latin".to_string(),
                        ..Default::default()
                    }],
                ],
                char_shapes: vec![RhwpCharShape {
                    font_ids: [9, 0, 0, 0, 0, 0, 0],
                    ..Default::default()
                }],
                ..Default::default()
            },
            sections: vec![RhwpSection {
                paragraphs: vec![RhwpParagraph {
                    text: "font".to_string(),
                    char_offsets: vec![0, 1, 2, 3],
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
        let paragraph = match &bridged.sections[0].blocks[0] {
            Block::Paragraph(paragraph) => paragraph,
            other => panic!("expected paragraph block, got {other:?}"),
        };
        let run = match &paragraph.inlines[0] {
            Inline::Text(run) => run,
            other => panic!("expected text run, got {other:?}"),
        };

        assert_eq!(run.style.font_family.as_deref(), Some("Fallback Latin"));
        assert!(bridged.warnings.iter().any(|warning| {
            warning
                .message
                .contains("missing font id 9 in font face group 0")
        }));
    }

    #[test]
    fn warns_when_paragraph_style_shape_refs_are_missing() {
        let document = RhwpDocument {
            doc_info: DocInfo {
                char_shapes: vec![RhwpCharShape::default()],
                para_shapes: vec![RhwpParaShape::default()],
                styles: vec![RhwpStyle {
                    local_name: "broken".to_string(),
                    para_shape_id: 5,
                    char_shape_id: 9,
                    ..Default::default()
                }],
                ..Default::default()
            },
            sections: vec![RhwpSection {
                paragraphs: vec![RhwpParagraph {
                    text: "styled".to_string(),
                    para_shape_id: 5,
                    style_id: 0,
                    char_offsets: vec![0, 1, 2, 3, 4, 5],
                    ..Default::default()
                }],
                ..Default::default()
            }],
            ..Default::default()
        };

        let bridged = BridgeContext::new(&document).into_document();

        assert!(bridged.warnings.iter().any(|warning| {
            warning
                .message
                .contains("paragraph style referenced missing para shape id 5")
        }));
        assert!(bridged.warnings.iter().any(|warning| {
            warning
                .message
                .contains("paragraph named style referenced missing char shape id 9")
        }));
        assert!(bridged.warnings.iter().any(|warning| {
            warning
                .message
                .contains("style sheet referenced missing para shape id 5")
        }));
        assert!(bridged.warnings.iter().any(|warning| {
            warning
                .message
                .contains("style sheet referenced missing char shape id 9")
        }));
    }

    #[test]
    fn preserves_percent_line_spacing() {
        let document = RhwpDocument {
            doc_info: DocInfo {
                para_shapes: vec![RhwpParaShape {
                    line_spacing_type: rhwp::model::style::LineSpacingType::Percent,
                    line_spacing: 160,
                    ..Default::default()
                }],
                ..Default::default()
            },
            sections: vec![RhwpSection {
                paragraphs: vec![RhwpParagraph {
                    text: "percent spacing".to_string(),
                    para_shape_id: 0,
                    ..Default::default()
                }],
                ..Default::default()
            }],
            ..Default::default()
        };

        let bridged = BridgeContext::new(&document).into_document();

        let Block::Paragraph(paragraph) = &bridged.sections[0].blocks[0] else {
            panic!("expected paragraph block");
        };
        assert_eq!(paragraph.style.spacing.line_percent, Some(Percent(160.0)));
        assert!(
            !bridged
                .warnings
                .iter()
                .any(|warning| warning.message.contains("percent line spacing"))
        );
    }

    #[test]
    fn warns_when_paragraph_style_values_are_approximated_or_omitted() {
        let document = RhwpDocument {
            doc_info: DocInfo {
                para_shapes: vec![RhwpParaShape {
                    alignment: RhwpAlignment::Distribute,
                    line_spacing_type: rhwp::model::style::LineSpacingType::SpaceOnly,
                    line_spacing: 1200,
                    tab_def_id: 3,
                    ..Default::default()
                }],
                ..Default::default()
            },
            sections: vec![RhwpSection {
                paragraphs: vec![RhwpParagraph {
                    text: "approximated paragraph".to_string(),
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
                assert_eq!(
                    paragraph.style.alignment,
                    Some(crate::ir::Alignment::Justify)
                );
                assert_eq!(paragraph.style.spacing.line_pt, Some(LengthPt(12.0)));
            }
            other => panic!("expected paragraph block, got {other:?}"),
        }
        for expected in [
            "alignment Distribute",
            "line spacing mode SpaceOnly",
            "tab definition id 3",
        ] {
            assert!(
                bridged
                    .warnings
                    .iter()
                    .any(|warning| warning.message.contains(expected)),
                "missing warning fragment {expected:?}: {:#?}",
                bridged.warnings
            );
        }
    }

    #[test]
    fn maps_paragraph_border_background_and_spacing() {
        let border = |line_type, width, color| RhwpBorderLine {
            line_type,
            width,
            color,
        };
        let document = RhwpDocument {
            doc_info: DocInfo {
                para_shapes: vec![RhwpParaShape {
                    attr1: (1 << 16) | (1 << 18),
                    attr2: (1 << 6) | (1 << 8),
                    border_fill_id: 1,
                    // rhwp order: left, right, top, bottom
                    border_spacing: [100, 200, 300, 400],
                    ..Default::default()
                }],
                border_fills: vec![RhwpBorderFill {
                    borders: [
                        border(RhwpBorderLineType::Solid, 1, 0x00332211),
                        border(RhwpBorderLineType::Dash, 1, 0),
                        border(RhwpBorderLineType::Dot, 1, 0),
                        border(RhwpBorderLineType::Double, 1, 0),
                    ],
                    fill: Fill {
                        fill_type: FillType::Solid,
                        solid: Some(SolidFill {
                            background_color: 0x00665544,
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
                    text: "framed".to_string(),
                    para_shape_id: 0,
                    ..Default::default()
                }],
                ..Default::default()
            }],
            ..Default::default()
        };

        let bridged = BridgeContext::new(&document).into_document();
        let style = match &bridged.sections[0].blocks[0] {
            Block::Paragraph(paragraph) => &paragraph.style,
            other => panic!("expected paragraph block, got {other:?}"),
        };

        assert_eq!(
            style.background_color,
            Some(Color {
                r: 0x44,
                g: 0x55,
                b: 0x66,
                a: 255,
            })
        );
        assert_eq!(style.padding_left_pt, Some(LengthPt(1.0)));
        assert_eq!(style.padding_right_pt, Some(LengthPt(2.0)));
        assert_eq!(style.padding_top_pt, Some(LengthPt(3.0)));
        assert_eq!(style.padding_bottom_pt, Some(LengthPt(4.0)));
        assert_eq!(
            style.border_left.as_ref().map(|border| border.style),
            Some(BorderStyle::Solid)
        );
        assert_eq!(
            style.border_right.as_ref().map(|border| border.style),
            Some(BorderStyle::Dashed)
        );
        assert_eq!(
            style.border_top.as_ref().map(|border| border.style),
            Some(BorderStyle::Dotted)
        );
        assert_eq!(
            style.border_bottom.as_ref().map(|border| border.style),
            Some(BorderStyle::Double)
        );
        assert!(style.widow_orphan);
        assert!(style.keep_with_next);
        assert!(style.keep_lines);
        assert!(style.page_break_before);
        assert!(!bridged.warnings.iter().any(|warning| {
            warning.message.contains("paragraph borders")
                || warning.message.contains("paragraph background")
        }));
    }

    #[test]
    fn warns_when_text_run_char_shape_ref_is_missing() {
        let document = RhwpDocument {
            doc_info: DocInfo {
                char_shapes: vec![RhwpCharShape::default()],
                ..Default::default()
            },
            sections: vec![RhwpSection {
                paragraphs: vec![RhwpParagraph {
                    text: "broken".to_string(),
                    char_offsets: vec![0, 1, 2, 3, 4, 5],
                    char_shapes: vec![CharShapeRef {
                        start_pos: 0,
                        char_shape_id: 7,
                    }],
                    ..Default::default()
                }],
                ..Default::default()
            }],
            ..Default::default()
        };

        let bridged = BridgeContext::new(&document).into_document();

        assert!(bridged.warnings.iter().any(|warning| {
            warning
                .message
                .contains("paragraph text run referenced missing char shape id 7")
        }));
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
    fn warns_when_table_background_fill_is_approximated_or_omitted() {
        let document = RhwpDocument {
            doc_info: DocInfo {
                border_fills: vec![
                    RhwpBorderFill {
                        fill: Fill {
                            fill_type: FillType::Solid,
                            solid: Some(SolidFill {
                                background_color: 0,
                                pattern_color: 0x00FFFFFF,
                                pattern_type: 1,
                            }),
                            alpha: 128,
                            ..Default::default()
                        },
                        ..Default::default()
                    },
                    RhwpBorderFill {
                        fill: Fill {
                            fill_type: FillType::Gradient,
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
                            border_fill_id: 2,
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
                        g: 0,
                        b: 0,
                        a: 255,
                    })
                );
                assert_eq!(table.rows[0].cells[0].style.background_color, None);
            }
            other => panic!("expected table block, got {other:?}"),
        }
        assert!(bridged.warnings.iter().any(|warning| {
            warning.message.contains("solid fill pattern type 1")
                && warning.message.contains("approximated")
        }));
        assert!(
            bridged
                .warnings
                .iter()
                .any(|warning| warning.message.contains("fill opacity 128"))
        );
        assert!(bridged.warnings.iter().any(|warning| {
            warning.message.contains("gradient fill") && warning.message.contains("omitted")
        }));
    }

    #[test]
    fn maps_black_color_and_omits_invalid_colorref_sentinels() {
        assert_eq!(
            color_ref_to_color_option(0),
            Some(Color {
                r: 0,
                g: 0,
                b: 0,
                a: 255,
            })
        );
        assert_eq!(color_ref_to_color_option(0xFFFFFFFF), None);
        assert_eq!(color_ref_to_color_option(0x01000000), None);
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
    fn warns_when_table_border_fill_refs_are_missing() {
        let document = RhwpDocument {
            sections: vec![RhwpSection {
                paragraphs: vec![RhwpParagraph {
                    controls: vec![Control::Table(Box::new(RhwpTable {
                        row_count: 1,
                        col_count: 1,
                        border_fill_id: 9,
                        cells: vec![RhwpCell {
                            row: 0,
                            col: 0,
                            border_fill_id: 7,
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

        assert!(bridged.warnings.iter().any(|warning| {
            warning
                .message
                .contains("table background referenced missing border fill id 9")
        }));
        assert!(bridged.warnings.iter().any(|warning| {
            warning
                .message
                .contains("table cell background referenced missing border fill id 7")
        }));
        assert!(bridged.warnings.iter().any(|warning| {
            warning
                .message
                .contains("table cell borders referenced missing border fill id 7")
        }));
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
                assert_eq!(paragraph.inlines.len(), 3);
                assert!(matches!(
                    paragraph.inlines.get(1),
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
        assert!(bridged.warnings.iter().any(|warning| {
            warning
                .message
                .contains("field controls could not be placed")
        }));
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
    fn orders_block_controls_around_paragraph_from_recovered_offsets() {
        let document = RhwpDocument {
            sections: vec![RhwpSection {
                paragraphs: vec![
                    RhwpParagraph {
                        char_count: 14,
                        text: "front".to_string(),
                        char_offsets: vec![8, 9, 10, 11, 12],
                        controls: vec![Control::Unknown(RhwpUnknownControl { ctrl_id: 1 })],
                        ..Default::default()
                    },
                    RhwpParagraph {
                        char_count: 13,
                        text: "back".to_string(),
                        char_offsets: vec![0, 1, 2, 3],
                        controls: vec![Control::Unknown(RhwpUnknownControl { ctrl_id: 2 })],
                        ..Default::default()
                    },
                ],
                ..Default::default()
            }],
            ..Default::default()
        };

        let bridged = BridgeContext::new(&document).into_document();
        let blocks = &bridged.sections[0].blocks;

        assert!(matches!(
            &blocks[0],
            Block::Unknown(unknown) if unknown.kind == "control:0x00000001"
        ));
        assert!(matches!(
            &blocks[1],
            Block::Paragraph(_)
                if crate::util::plain_text::blocks_to_plain_text(
                    std::slice::from_ref(&blocks[1])
                ) == "front"
        ));
        assert!(matches!(
            &blocks[2],
            Block::Paragraph(_)
                if crate::util::plain_text::blocks_to_plain_text(
                    std::slice::from_ref(&blocks[2])
                ) == "back"
        ));
        assert!(matches!(
            &blocks[3],
            Block::Unknown(unknown) if unknown.kind == "control:0x00000002"
        ));
        assert!(
            !bridged
                .warnings
                .iter()
                .any(|warning| warning.message.contains("exact reading order may differ"))
        );
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
    fn places_note_reference_at_recovered_control_offset() {
        let document = RhwpDocument {
            sections: vec![RhwpSection {
                paragraphs: vec![RhwpParagraph {
                    char_count: 20,
                    text: "beforeafter".to_string(),
                    char_offsets: vec![0, 1, 2, 3, 4, 5, 14, 15, 16, 17, 18],
                    controls: vec![Control::Footnote(Box::new(RhwpFootnote {
                        number: 1,
                        paragraphs: vec![RhwpParagraph {
                            text: "note".to_string(),
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
        let paragraph = match &bridged.sections[0].blocks[0] {
            Block::Paragraph(paragraph) => paragraph,
            other => panic!("expected paragraph block, got {other:?}"),
        };

        assert_eq!(paragraph.inlines.len(), 3);
        assert!(matches!(
            &paragraph.inlines[0],
            Inline::Text(run) if run.text == "before"
        ));
        assert!(matches!(
            &paragraph.inlines[1],
            Inline::FootnoteRef { note_id } if note_id.as_str() == "footnote-1"
        ));
        assert!(matches!(
            &paragraph.inlines[2],
            Inline::Text(run) if run.text == "after"
        ));
        assert!(
            !bridged
                .warnings
                .iter()
                .any(|warning| { warning.message.contains("positions could not be recovered") })
        );
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
    fn warns_when_bullet_metadata_cannot_be_modeled() {
        let document = RhwpDocument {
            doc_info: DocInfo {
                para_shapes: vec![RhwpParaShape {
                    head_type: RhwpHeadType::Bullet,
                    numbering_id: 1,
                    ..Default::default()
                }],
                bullets: vec![RhwpBullet {
                    attr: 3,
                    width_adjust: 100,
                    text_distance: 200,
                    bullet_char: '•',
                    image_bullet: 7,
                    image_data: [1, 2, 3, 4],
                    check_bullet_char: '✓',
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

        assert!(bridged.warnings.iter().any(|warning| {
            warning.message.contains("used image bullet 7")
                && warning
                    .message
                    .contains("omitted the image bullet resource")
        }));
        assert!(bridged.warnings.iter().any(|warning| {
            warning.message.contains("width_adjust=100")
                && warning.message.contains("text_distance=200")
        }));
        assert!(bridged.warnings.iter().any(|warning| {
            warning.message.contains("check-bullet marker") && warning.message.contains('✓')
        }));
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
    fn preserves_ordered_list_format_and_warns_for_unmodeled_metadata() {
        let mut numbering = RhwpNumbering {
            level_start_numbers: [1, 1, 1, 1, 1, 1, 1],
            ..Default::default()
        };
        numbering.level_formats[0] = "제^1장".to_string();
        numbering.heads[0].attr = 5;
        numbering.heads[0].width_adjust = 100;
        numbering.heads[0].text_distance = 200;
        numbering.heads[0].char_shape_id = 3;
        numbering.heads[0].number_format = 2;

        let document = RhwpDocument {
            doc_info: DocInfo {
                para_shapes: vec![RhwpParaShape {
                    head_type: RhwpHeadType::Number,
                    numbering_id: 1,
                    ..Default::default()
                }],
                numberings: vec![numbering],
                ..Default::default()
            },
            sections: vec![RhwpSection {
                paragraphs: vec![RhwpParagraph {
                    text: "chapter".to_string(),
                    para_shape_id: 0,
                    ..Default::default()
                }],
                ..Default::default()
            }],
            ..Default::default()
        };

        let bridged = BridgeContext::new(&document).into_document();
        let list = match &bridged.sections[0].blocks[0] {
            Block::Paragraph(paragraph) => paragraph.list.as_ref().expect("list info"),
            other => panic!("expected paragraph block, got {other:?}"),
        };

        assert_eq!(list.marker.as_deref(), Some("제I장"));
        assert_eq!(list.marker_format.as_deref(), Some("제^1장"));
        assert!(!bridged.warnings.iter().any(|warning| {
            warning.message.contains("format \"제^1장\"")
                && warning.message.contains("plain number")
        }));
        assert!(bridged.warnings.iter().any(|warning| {
            warning.message.contains("char_shape_id=3")
                && warning.message.contains("layout/style metadata")
        }));
    }

    #[test]
    fn expands_multilevel_ordered_markers_from_numbering_state() {
        let mut numbering = RhwpNumbering {
            level_start_numbers: [1, 1, 1, 1, 1, 1, 1],
            ..Default::default()
        };
        numbering.level_formats[0] = "^1.".to_string();
        numbering.level_formats[1] = "^1-^2)".to_string();
        numbering.heads[0].number_format = 0;
        numbering.heads[1].number_format = 4;

        let document = RhwpDocument {
            doc_info: DocInfo {
                para_shapes: vec![
                    RhwpParaShape {
                        head_type: RhwpHeadType::Number,
                        numbering_id: 1,
                        para_level: 0,
                        ..Default::default()
                    },
                    RhwpParaShape {
                        head_type: RhwpHeadType::Number,
                        numbering_id: 1,
                        para_level: 1,
                        ..Default::default()
                    },
                ],
                numberings: vec![numbering],
                ..Default::default()
            },
            sections: vec![RhwpSection {
                paragraphs: vec![
                    RhwpParagraph {
                        text: "chapter".to_string(),
                        para_shape_id: 0,
                        ..Default::default()
                    },
                    RhwpParagraph {
                        text: "section".to_string(),
                        para_shape_id: 1,
                        ..Default::default()
                    },
                ],
                ..Default::default()
            }],
            ..Default::default()
        };

        let bridged = BridgeContext::new(&document).into_document();
        let lists = bridged.sections[0]
            .blocks
            .iter()
            .filter_map(|block| match block {
                Block::Paragraph(paragraph) => paragraph.list.as_ref(),
                _ => None,
            })
            .collect::<Vec<_>>();

        assert_eq!(lists[0].marker.as_deref(), Some("1."));
        assert_eq!(lists[1].marker.as_deref(), Some("1-A)"));
        assert_eq!(lists[1].marker_format.as_deref(), Some("^1-^2)"));
        assert_eq!(lists[1].number, Some(1));
    }

    #[test]
    fn warns_when_list_definitions_are_missing() {
        let document = RhwpDocument {
            doc_info: DocInfo {
                para_shapes: vec![
                    RhwpParaShape {
                        head_type: RhwpHeadType::Bullet,
                        para_level: 0,
                        numbering_id: 3,
                        ..Default::default()
                    },
                    RhwpParaShape {
                        head_type: RhwpHeadType::Number,
                        para_level: 0,
                        numbering_id: 4,
                        ..Default::default()
                    },
                ],
                ..Default::default()
            },
            sections: vec![RhwpSection {
                paragraphs: vec![
                    RhwpParagraph {
                        text: "bullet".to_string(),
                        para_shape_id: 0,
                        ..Default::default()
                    },
                    RhwpParagraph {
                        text: "number".to_string(),
                        para_shape_id: 1,
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
                let list = paragraph.list.as_ref().expect("list info should exist");
                assert_eq!(list.kind, ListKind::Unordered);
                assert_eq!(list.marker, None);
            }
            other => panic!("expected paragraph block, got {other:?}"),
        }

        match &bridged.sections[0].blocks[1] {
            Block::Paragraph(paragraph) => {
                let list = paragraph.list.as_ref().expect("list info should exist");
                assert_eq!(list.kind, ListKind::Ordered);
                assert_eq!(list.number, Some(1));
            }
            other => panic!("expected paragraph block, got {other:?}"),
        }

        assert!(
            bridged
                .warnings
                .iter()
                .any(|warning| { warning.message.contains("referenced missing bullet id 3") })
        );
        assert!(bridged.warnings.iter().any(|warning| {
            warning
                .message
                .contains("referenced missing numbering id 4")
        }));
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
