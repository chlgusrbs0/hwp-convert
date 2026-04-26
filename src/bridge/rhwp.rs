use std::error::Error;
use std::io;
use std::path::Path;

use rhwp::model::control::{Control, Equation as RhwpEquation};
use rhwp::model::document::{Document as RhwpDocument, Section as RhwpSection};
use rhwp::model::image::Picture;
use rhwp::model::paragraph::{CharShapeRef, Paragraph as RhwpParagraph};
use rhwp::model::shape::ShapeObject;
use rhwp::model::style::{
    Alignment as RhwpAlignment, BorderFill as RhwpBorderFill, CharShape as RhwpCharShape,
    FillType as RhwpFillType, ParaShape as RhwpParaShape, UnderlineType as RhwpUnderlineType,
};
use rhwp::model::table::{Cell as RhwpCell, Table as RhwpTable};

use crate::hwpx::{self, InputKind};
use crate::ir::{
    Block, Color, ConversionWarning, Document, Equation, EquationKind, Image, ImageResource,
    Inline, LengthPt, LengthPx, NamedParagraphStyle, NamedTextStyle, Paragraph, ParagraphRole,
    ParagraphStyle, ParagraphStyleId, Resource, ResourceId, ResourceStore, Section, Shape,
    ShapeKind, Spacing, StyleSheet, Table, TableCell, TableCellStyle, TableRow, TableStyle,
    TextRun, TextStyle, TextStyleId, WarningCode,
};

/// Parse a source document with `rhwp` and bridge the resulting model into the
/// local `Document` IR. For `.hwpx`, preview text fallback remains available
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
                fallback_to_hwpx_preview(&bytes, &empty_error).map_err(Into::into)
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
                fallback_to_hwpx_preview(&bytes, &rhwp_error).map_err(Into::into)
            } else {
                Err(rhwp_error.into())
            }
        }
    }
}

fn fallback_to_hwpx_preview(bytes: &[u8], source_error: &io::Error) -> io::Result<Document> {
    let paragraphs = hwpx::read_preview_text_from_archive(bytes)
        .map_err(|fallback_error| hwpx::combine_hwpx_errors(source_error, &fallback_error))?;

    Ok(document_from_preview_paragraphs(paragraphs))
}

fn document_from_preview_paragraphs(paragraphs: Vec<String>) -> Document {
    let mut document = Document::from_paragraphs(paragraphs);
    document.warnings.push(ConversionWarning {
        code: WarningCode::UsedHwpxPreviewFallback,
        message: "Used HWPX preview fallback. Preview/PrvText.txt only recovers plain text, so table, image, and style data may be missing.".to_string(),
    });
    document
}

fn document_has_blocks(document: &Document) -> bool {
    document
        .sections
        .iter()
        .any(|section| !section.blocks.is_empty())
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
}

impl<'a> BridgeContext<'a> {
    fn new(source: &'a RhwpDocument) -> Self {
        Self {
            source,
            resources: ResourceStore::default(),
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
            notes: crate::ir::NoteStore::default(),
            warnings: Vec::new(),
        }
    }

    fn map_section(&mut self, section: &RhwpSection) -> Section {
        let mut blocks = Vec::new();

        for paragraph in &section.paragraphs {
            self.append_blocks_from_paragraph(&mut blocks, paragraph);
        }

        Section {
            blocks,
            ..Default::default()
        }
    }

    fn append_blocks_from_paragraph(&mut self, blocks: &mut Vec<Block>, paragraph: &RhwpParagraph) {
        if let Some(mapped) = self.map_paragraph(paragraph) {
            blocks.push(Block::Paragraph(mapped));
        }

        for control in &paragraph.controls {
            if let Some(block) = self.map_control_block(control) {
                blocks.push(block);
            }
        }
    }

    fn map_paragraph(&self, paragraph: &RhwpParagraph) -> Option<Paragraph> {
        let inlines = self.map_paragraph_inlines(paragraph);
        if inlines.is_empty() {
            return None;
        }

        Some(Paragraph {
            role: ParagraphRole::Body,
            inlines,
            style: self.map_paragraph_style_by_id(paragraph.para_shape_id),
            style_ref: self.paragraph_style_ref(paragraph),
            list: None,
        })
    }

    fn map_paragraph_inlines(&self, paragraph: &RhwpParagraph) -> Vec<Inline> {
        if paragraph.text.is_empty() {
            return Vec::new();
        }

        let chars: Vec<char> = paragraph.text.chars().collect();
        let segments = self.build_text_segments(paragraph, chars.len());
        let mut inlines = Vec::new();

        for segment in segments {
            if segment.start >= segment.end || segment.end > chars.len() {
                continue;
            }

            let text: String = chars[segment.start..segment.end].iter().collect();
            push_text_fragment(
                &mut inlines,
                &text,
                &segment.style,
                segment.style_ref.as_ref(),
            );
        }

        inlines
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

    fn map_control_block(&mut self, control: &Control) -> Option<Block> {
        match control {
            Control::Table(table) => Some(Block::Table(self.map_table(table))),
            Control::Picture(picture) => Some(self.map_picture_block(picture)),
            Control::Equation(equation) => Some(Block::Equation(self.map_equation(equation))),
            Control::Shape(shape) => Some(Block::Shape(self.map_shape(shape))),
            Control::Unknown(control) => Some(Block::Unknown(crate::ir::UnknownBlock {
                kind: format!("control:{:#010x}", control.ctrl_id),
                fallback_text: None,
                message: Some("rhwp exposed this control as Unknown".to_string()),
                source: Some("rhwp".to_string()),
            })),
            _ => None,
        }
    }

    fn map_table(&mut self, table: &RhwpTable) -> Table {
        let mut rows = Vec::new();

        for row_index in 0..table.row_count {
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

    fn map_table_cell(&mut self, cell: &RhwpCell) -> TableCell {
        let mut blocks = Vec::new();

        for paragraph in &cell.paragraphs {
            self.append_blocks_from_paragraph(&mut blocks, paragraph);
        }

        TableCell {
            row_span: cell.row_span as u32,
            col_span: cell.col_span as u32,
            blocks,
            style: TableCellStyle {
                background_color: self.border_fill_background_color(cell.border_fill_id),
            },
        }
    }

    fn map_picture_block(&mut self, picture: &Picture) -> Block {
        match self.ensure_image_resource(picture.image_attr.bin_data_id) {
            Some(resource_id) => Block::Image(Image {
                resource_id,
                alt: non_empty_string(&picture.common.description),
                caption: self
                    .caption_text(picture.caption.as_ref().map(|caption| &caption.paragraphs)),
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

    fn map_equation(&self, equation: &RhwpEquation) -> Equation {
        let content = non_empty_string(&equation.script);
        Equation {
            kind: EquationKind::PlainText,
            fallback_text: content.clone().or_else(|| Some("[equation]".to_string())),
            content,
            resource_id: None,
        }
    }

    fn map_shape(&self, shape: &ShapeObject) -> Shape {
        let kind = match shape {
            ShapeObject::Line(_) => ShapeKind::Line,
            ShapeObject::Rectangle(_) => ShapeKind::Rectangle,
            ShapeObject::Ellipse(_) | ShapeObject::Arc(_) => ShapeKind::Ellipse,
            ShapeObject::Polygon(_) | ShapeObject::Curve(_) => ShapeKind::Polygon,
            ShapeObject::Group(_) | ShapeObject::Picture(_) => ShapeKind::Unknown,
        };
        let description = non_empty_string(&shape.common().description);

        Shape {
            kind,
            fallback_text: description.clone().or_else(|| Some("[shape]".to_string())),
            description,
        }
    }

    fn ensure_image_resource(&mut self, bin_data_id: u16) -> Option<ResourceId> {
        let resource_id = ResourceId(format!("image-{bin_data_id}"));
        if self.resources.get(&resource_id).is_some() {
            return Some(resource_id);
        }

        let bin_data = self
            .source
            .bin_data_content
            .iter()
            .find(|bin_data| bin_data.id == bin_data_id)?;
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
        self.source
            .doc_info
            .border_fills
            .get(border_fill_id as usize)
    }

    fn caption_text(&self, paragraphs: Option<&Vec<RhwpParagraph>>) -> Option<String> {
        let paragraphs = paragraphs?;
        let lines = paragraphs
            .iter()
            .map(|paragraph| paragraph.text.trim())
            .filter(|text| !text.is_empty())
            .collect::<Vec<_>>();

        if lines.is_empty() {
            None
        } else {
            Some(lines.join("\n"))
        }
    }
}

#[derive(Clone)]
struct TextSegment {
    start: usize,
    end: usize,
    style: TextStyle,
    style_ref: Option<TextStyleId>,
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

    use rhwp::model::bin_data::BinDataContent;
    use rhwp::model::document::{DocInfo, Document as RhwpDocument, Section as RhwpSection};
    use rhwp::model::image::{ImageAttr, Picture};
    use rhwp::model::paragraph::{CharShapeRef, Paragraph as RhwpParagraph};
    use rhwp::model::style::{
        Alignment as RhwpAlignment, BorderFill as RhwpBorderFill, CharShape as RhwpCharShape, Fill,
        FillType, Font, ParaShape as RhwpParaShape, SolidFill, Style as RhwpStyle,
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
    fn preview_fallback_marks_warning() {
        let document = document_from_preview_paragraphs(vec!["preview".to_string()]);

        assert_eq!(document.sections.len(), 1);
        assert_eq!(document.warnings.len(), 1);
        assert_eq!(
            document.warnings[0].code,
            WarningCode::UsedHwpxPreviewFallback
        );
    }
}
