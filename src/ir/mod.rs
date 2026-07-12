use std::error::Error;
use std::fmt;

use serde::{Deserialize, Serialize};

/// Serialized Document IR format version.
///
/// This is independent from the internal roadmap milestones (`v0`-`v7`).
/// Bump this when JSON compatibility changes, such as new enum variants,
/// new required fields, or other output-shape changes.
///
/// v8: added `TextStyle` decoration fields (superscript, subscript,
/// emphasis_dot, emboss, engrave, outline, shadow). All are additive and
/// `#[serde(default)]`, so older JSON still deserializes.
/// v9: added `TableCell::is_header` and `TableCellStyle::vertical_align`.
/// Additive and `#[serde(default)]`.
/// v10: added `TextStyle::{underline_color, strike_color}` and
/// `TableCellStyle::{width, height}`. Additive and `#[serde(default)]`.
/// v11: added `TableCellStyle` padding fields. Additive and `#[serde(default)]`.
/// v12: added `TableCellStyle` per-side borders (`Border`/`BorderStyle`).
/// Additive and `#[serde(default)]`.
/// v13: added `Image::{border, grayscale}`. Additive and `#[serde(default)]`.
/// v22: added `Shape::{border, background_color}`. Additive and `#[serde(default)]`.
/// v23: added `Shape` rotation and flip fields. Additive and `#[serde(default)]`.
/// v24: added `Shape` text-box padding and vertical alignment. Additive and
/// `#[serde(default)]`.
/// v25: added `TableStyle::cell_spacing`. Additive and `#[serde(default)]`.
/// v26: added `Image` padding fields. Additive and `#[serde(default)]`.
/// v27: added `Image::caption_placement`. Additive and `#[serde(default)]`.
pub const IR_VERSION: u16 = 32;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Document {
    pub ir_version: u16,
    pub metadata: Metadata,
    pub sections: Vec<Section>,
    /// Additive IR fields should use `#[serde(default)]` so older serialized
    /// documents can still be deserialized after new optional structure is added.
    #[serde(default)]
    pub resources: ResourceStore,
    #[serde(default)]
    pub styles: StyleSheet,
    #[serde(default)]
    pub notes: NoteStore,
    #[serde(default)]
    pub warnings: Vec<ConversionWarning>,
}

impl Default for Document {
    fn default() -> Self {
        Self {
            ir_version: IR_VERSION,
            metadata: Metadata::default(),
            sections: Vec::new(),
            resources: ResourceStore::default(),
            styles: StyleSheet::default(),
            notes: NoteStore::default(),
            warnings: Vec::new(),
        }
    }
}

impl Document {
    pub fn from_paragraphs(paragraphs: Vec<String>) -> Self {
        let blocks = paragraphs
            .into_iter()
            .map(|text| Block::Paragraph(Paragraph::from_plain_text(text)))
            .collect();

        Self {
            ir_version: IR_VERSION,
            metadata: Metadata::default(),
            sections: vec![Section {
                blocks,
                ..Default::default()
            }],
            resources: ResourceStore::default(),
            styles: StyleSheet::default(),
            notes: NoteStore::default(),
            warnings: Vec::new(),
        }
    }
}

impl Paragraph {
    pub fn from_plain_text(text: String) -> Self {
        Self {
            role: ParagraphRole::Body,
            inlines: inlines_from_plain_text(text),
            style: ParagraphStyle::default(),
            style_ref: None,
            list: None,
        }
    }
}

fn inlines_from_plain_text(text: String) -> Vec<Inline> {
    let mut inlines = Vec::new();
    let mut text_buffer = String::new();

    for ch in text.chars() {
        match ch {
            '\n' => {
                push_plain_text_run(&mut inlines, &mut text_buffer);
                inlines.push(Inline::LineBreak);
            }
            '\t' => {
                push_plain_text_run(&mut inlines, &mut text_buffer);
                inlines.push(Inline::Tab);
            }
            _ => text_buffer.push(ch),
        }
    }

    push_plain_text_run(&mut inlines, &mut text_buffer);
    inlines
}

fn push_plain_text_run(inlines: &mut Vec<Inline>, text_buffer: &mut String) {
    if text_buffer.is_empty() {
        return;
    }

    inlines.push(Inline::Text(TextRun {
        text: std::mem::take(text_buffer),
        style: TextStyle::default(),
        style_ref: None,
    }));
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct Metadata {
    pub title: Option<String>,
    pub author: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct Section {
    pub blocks: Vec<Block>,
    #[serde(default)]
    pub headers: Vec<HeaderFooter>,
    #[serde(default)]
    pub footers: Vec<HeaderFooter>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Block {
    Paragraph(Paragraph),
    Table(Table),
    Image(Image),
    Equation(Equation),
    Shape(Shape),
    Chart(Chart),
    Unknown(UnknownBlock),
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct Paragraph {
    pub role: ParagraphRole,
    pub inlines: Vec<Inline>,
    #[serde(default)]
    pub style: ParagraphStyle,
    #[serde(default)]
    pub style_ref: Option<ParagraphStyleId>,
    #[serde(default)]
    pub list: Option<ListInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(tag = "role", rename_all = "snake_case")]
pub enum ParagraphRole {
    #[default]
    Body,
    Heading {
        level: u8,
    },
    Title,
    Caption,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Inline {
    Text(TextRun),
    LineBreak,
    Tab,
    Anchor { id: String },
    Link(Link),
    FootnoteRef { note_id: NoteId },
    EndnoteRef { note_id: NoteId },
    Unknown(UnknownInline),
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct TextRun {
    pub text: String,
    #[serde(default)]
    pub style: TextStyle,
    #[serde(default)]
    pub style_ref: Option<TextStyleId>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default, PartialEq, PartialOrd)]
#[serde(transparent)]
pub struct LengthPt(pub f32);

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default, PartialEq, PartialOrd)]
#[serde(transparent)]
pub struct LengthMm(pub f32);

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default, PartialEq, PartialOrd)]
#[serde(transparent)]
pub struct LengthPx(pub f32);

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default, PartialEq, PartialOrd)]
#[serde(transparent)]
pub struct Percent(pub f32);

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    #[serde(default = "default_alpha")]
    pub a: u8,
}

impl Default for Color {
    fn default() -> Self {
        Self {
            r: 0,
            g: 0,
            b: 0,
            a: default_alpha(),
        }
    }
}

const fn default_alpha() -> u8 {
    255
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
#[serde(default)]
pub struct TextStyle {
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub strike: bool,
    pub superscript: bool,
    pub subscript: bool,
    /// Korean emphasis dots rendered above/below glyphs (한글 강조점).
    pub emphasis_dot: bool,
    pub emboss: bool,
    pub engrave: bool,
    pub outline: bool,
    pub shadow: bool,
    pub font_family: Option<String>,
    /// Typographic size in points (pt).
    #[serde(alias = "font_size")]
    pub font_size_pt: Option<LengthPt>,
    pub color: Option<Color>,
    pub background_color: Option<Color>,
    pub underline_color: Option<Color>,
    pub strike_color: Option<Color>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub underline_style: Option<TextDecorationStyle>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub strike_style: Option<TextDecorationStyle>,
    #[serde(skip_serializing_if = "is_false")]
    pub underline_above: bool,
    /// Glyph width relative to the selected font's normal width.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub font_width_percent: Option<Percent>,
    /// Additional spacing between glyphs, relative to the effective font size.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub letter_spacing_percent: Option<Percent>,
    /// Source-relative size metadata; mapped `font_size_pt` is already effective.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub relative_size_percent: Option<Percent>,
    /// Baseline offset relative to the effective font size; positive values move up.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vertical_offset_percent: Option<Percent>,
    #[serde(skip_serializing_if = "is_false")]
    pub kerning: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TextDecorationStyle {
    Solid,
    Dashed,
    Dotted,
    Double,
    Wavy,
}

fn is_false(value: &bool) -> bool {
    !*value
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
#[serde(default)]
pub struct ParagraphStyle {
    pub alignment: Option<Alignment>,
    pub spacing: Spacing,
    pub indent: Indent,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub background_color: Option<Color>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub padding_top_pt: Option<LengthPt>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub padding_right_pt: Option<LengthPt>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub padding_bottom_pt: Option<LengthPt>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub padding_left_pt: Option<LengthPt>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub border_top: Option<Border>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub border_right: Option<Border>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub border_bottom: Option<Border>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub border_left: Option<Border>,
    #[serde(skip_serializing_if = "is_false")]
    pub widow_orphan: bool,
    #[serde(skip_serializing_if = "is_false")]
    pub keep_with_next: bool,
    #[serde(skip_serializing_if = "is_false")]
    pub keep_lines: bool,
    #[serde(skip_serializing_if = "is_false")]
    pub page_break_before: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Alignment {
    Left,
    Center,
    Right,
    Justify,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum VerticalAlign {
    Top,
    Middle,
    Bottom,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
#[serde(default)]
pub struct Spacing {
    pub before_pt: Option<LengthPt>,
    pub after_pt: Option<LengthPt>,
    pub line_pt: Option<LengthPt>,
    pub line_percent: Option<Percent>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
#[serde(default)]
pub struct Indent {
    pub left_pt: Option<LengthPt>,
    pub right_pt: Option<LengthPt>,
    pub first_line_pt: Option<LengthPt>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
#[serde(default)]
pub struct StyleSheet {
    pub text_styles: Vec<NamedTextStyle>,
    pub paragraph_styles: Vec<NamedParagraphStyle>,
    pub table_styles: Vec<NamedTableStyle>,
    pub table_cell_styles: Vec<NamedTableCellStyle>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(transparent)]
pub struct TextStyleId(pub String);

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(transparent)]
pub struct ParagraphStyleId(pub String);

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(transparent)]
pub struct TableStyleId(pub String);

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(transparent)]
pub struct TableCellStyleId(pub String);

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct NamedTextStyle {
    pub id: TextStyleId,
    pub name: Option<String>,
    pub style: TextStyle,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct NamedParagraphStyle {
    pub id: ParagraphStyleId,
    pub name: Option<String>,
    pub style: ParagraphStyle,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct NamedTableStyle {
    pub id: TableStyleId,
    pub name: Option<String>,
    pub style: TableStyle,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct NamedTableCellStyle {
    pub id: TableCellStyleId,
    pub name: Option<String>,
    pub style: TableCellStyle,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
#[serde(default)]
pub struct NoteStore {
    pub notes: Vec<Note>,
}

impl NoteStore {
    pub fn get(&self, note_id: &NoteId) -> Option<&Note> {
        self.notes.iter().find(|note| &note.id == note_id)
    }

    pub fn insert_unique(&mut self, note: Note) -> Result<(), DuplicateNoteIdError> {
        let note_id = note.id.clone();
        if self.get(&note_id).is_some() {
            return Err(DuplicateNoteIdError { note_id });
        }

        self.notes.push(note);
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(transparent)]
pub struct NoteId(pub String);

impl NoteId {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DuplicateNoteIdError {
    pub note_id: NoteId,
}

impl fmt::Display for DuplicateNoteIdError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "duplicate note id: {}", self.note_id.as_str())
    }
}

impl Error for DuplicateNoteIdError {}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Note {
    pub id: NoteId,
    pub kind: NoteKind,
    pub blocks: Vec<Block>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum NoteKind {
    Footnote,
    Endnote,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct HeaderFooter {
    pub placement: HeaderFooterPlacement,
    pub blocks: Vec<Block>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum HeaderFooterPlacement {
    #[default]
    Default,
    FirstPage,
    OddPage,
    EvenPage,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct Link {
    pub url: String,
    pub title: Option<String>,
    pub inlines: Vec<Inline>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct ListInfo {
    pub kind: ListKind,
    pub level: u8,
    /// Rendered marker for this concrete list item.
    pub marker: Option<String>,
    /// Source numbering template such as `^1.` when available.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub marker_format: Option<String>,
    pub number: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ListKind {
    Ordered,
    Unordered,
    #[default]
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct ResourceStore {
    /// Preserve insertion order for serialized output; use `insert_unique` to
    /// maintain the no-duplicate-`ResourceId` invariant.
    #[serde(default)]
    pub entries: Vec<Resource>,
}

impl ResourceStore {
    pub fn get(&self, resource_id: &ResourceId) -> Option<&Resource> {
        self.entries
            .iter()
            .find(|resource| resource.id() == resource_id)
    }

    pub fn insert_unique(&mut self, resource: Resource) -> Result<(), DuplicateResourceIdError> {
        let resource_id = resource.id().clone();
        if self.get(&resource_id).is_some() {
            return Err(DuplicateResourceIdError { resource_id });
        }

        self.entries.push(resource);
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Resource {
    Image(ImageResource),
    Binary(BinaryResource),
}

impl Resource {
    pub fn id(&self) -> &ResourceId {
        match self {
            Resource::Image(resource) => &resource.id,
            Resource::Binary(resource) => &resource.id,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(transparent)]
pub struct ResourceId(pub String);

impl ResourceId {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DuplicateResourceIdError {
    pub resource_id: ResourceId,
}

impl fmt::Display for DuplicateResourceIdError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "duplicate resource id: {}", self.resource_id.as_str())
    }
}

impl Error for DuplicateResourceIdError {}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct ImageResource {
    pub id: ResourceId,
    pub media_type: Option<String>,
    pub extension: Option<String>,
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct BinaryResource {
    pub id: ResourceId,
    pub media_type: Option<String>,
    pub extension: Option<String>,
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct Image {
    pub resource_id: ResourceId,
    pub alt: Option<String>,
    pub caption: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub caption_placement: Option<CaptionPlacement>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub crop: Option<ImageCrop>,
    /// Display hint in px until Layout IR defines document-space units.
    pub width: Option<LengthPx>,
    /// Display hint in px until Layout IR defines document-space units.
    pub height: Option<LengthPx>,
    /// Uniform border around the image, if any.
    #[serde(default)]
    pub border: Option<Border>,
    /// Grayscale/black-and-white rendering effect.
    #[serde(default)]
    pub grayscale: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effect: Option<ImageEffect>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub placement: Option<ImagePlacement>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub brightness: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub contrast: Option<i32>,
    /// Display opacity in the inclusive range 0.0..=1.0.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub opacity: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rotation_degrees: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub flip_horizontal: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub flip_vertical: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub padding_top: Option<LengthPx>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub padding_right: Option<LengthPx>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub padding_bottom: Option<LengthPx>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub padding_left: Option<LengthPx>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CaptionPlacement {
    Left,
    Right,
    Top,
    Bottom,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ImageEffect {
    Grayscale,
    BlackWhite,
    Pattern8x8,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub struct ImagePlacement {
    pub treat_as_character: bool,
    pub text_wrap: ImageTextWrap,
    pub vertical_relative_to: VerticalRelativeTo,
    pub vertical_alignment: VerticalObjectAlignment,
    pub vertical_offset: LengthPx,
    pub horizontal_relative_to: HorizontalRelativeTo,
    pub horizontal_alignment: HorizontalObjectAlignment,
    pub horizontal_offset: LengthPx,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ImageTextWrap {
    Square,
    Tight,
    Through,
    TopAndBottom,
    BehindText,
    InFrontOfText,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum VerticalRelativeTo {
    Paper,
    Page,
    Paragraph,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum HorizontalRelativeTo {
    Paper,
    Page,
    Column,
    Paragraph,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum VerticalObjectAlignment {
    Top,
    Center,
    Bottom,
    Inside,
    Outside,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum HorizontalObjectAlignment {
    Left,
    Center,
    Right,
    Inside,
    Outside,
}

/// Source-image crop rectangle. Coordinates are measured from the source
/// image's top-left corner and use the same px conversion as other HWPUNIT
/// dimensions in Document IR.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub struct ImageCrop {
    pub left: LengthPx,
    pub top: LengthPx,
    pub right: LengthPx,
    pub bottom: LengthPx,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_width: Option<LengthPx>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_height: Option<LengthPx>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct Equation {
    pub kind: EquationKind,
    pub content: Option<String>,
    pub fallback_text: Option<String>,
    pub resource_id: Option<ResourceId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub font_size_pt: Option<LengthPt>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<Color>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub baseline_pt: Option<LengthPt>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub font_family: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub width: Option<LengthPx>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub height: Option<LengthPx>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub offset_x: Option<LengthPx>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub offset_y: Option<LengthPx>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EquationKind {
    PlainText,
    Latex,
    MathMl,
    #[default]
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct Shape {
    pub kind: ShapeKind,
    pub fallback_text: Option<String>,
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub border: Option<Border>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub background_color: Option<Color>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rotation_degrees: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub flip_horizontal: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub flip_vertical: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text_vertical_align: Option<VerticalAlign>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub padding_top: Option<LengthPx>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub padding_right: Option<LengthPx>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub padding_bottom: Option<LengthPx>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub padding_left: Option<LengthPx>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub width: Option<LengthPx>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub height: Option<LengthPx>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub offset_x: Option<LengthPx>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub offset_y: Option<LengthPx>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ShapeKind {
    Line,
    Rectangle,
    Ellipse,
    Polygon,
    TextBox,
    #[default]
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct Chart {
    pub title: Option<String>,
    pub fallback_text: Option<String>,
    pub resource_id: Option<ResourceId>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct Table {
    pub rows: Vec<TableRow>,
    pub style: TableStyle,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct TableRow {
    pub cells: Vec<TableCell>,
    pub height: Option<LengthPx>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TableCell {
    pub row_span: u32,
    pub col_span: u32,
    #[serde(default)]
    pub is_header: bool,
    pub blocks: Vec<Block>,
    pub style: TableCellStyle,
}

impl Default for TableCell {
    fn default() -> Self {
        Self {
            row_span: 1,
            col_span: 1,
            is_header: false,
            blocks: Vec::new(),
            style: TableCellStyle::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
#[serde(default)]
pub struct TableStyle {
    pub background_color: Option<Color>,
    pub width: Option<LengthPx>,
    pub height: Option<LengthPx>,
    pub margin_top: Option<LengthPx>,
    pub margin_right: Option<LengthPx>,
    pub margin_bottom: Option<LengthPx>,
    pub margin_left: Option<LengthPx>,
    pub cell_spacing: Option<LengthPx>,
    pub repeat_header: bool,
    pub page_break: Option<TablePageBreak>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TablePageBreak {
    Cell,
    Row,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
#[serde(default)]
pub struct TableCellStyle {
    pub background_color: Option<Color>,
    pub vertical_align: Option<VerticalAlign>,
    pub width: Option<LengthPx>,
    pub height: Option<LengthPx>,
    pub padding_top: Option<LengthPx>,
    pub padding_right: Option<LengthPx>,
    pub padding_bottom: Option<LengthPx>,
    pub padding_left: Option<LengthPx>,
    pub border_top: Option<Border>,
    pub border_right: Option<Border>,
    pub border_bottom: Option<Border>,
    pub border_left: Option<Border>,
}

/// A single border edge: used by table cells (per side) and images (uniform).
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct Border {
    pub width: LengthPx,
    pub style: BorderStyle,
    pub color: Option<Color>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BorderStyle {
    #[default]
    Solid,
    Dashed,
    Dotted,
    Double,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(default)]
pub struct UnknownBlock {
    pub kind: String,
    pub fallback_text: Option<String>,
    pub message: Option<String>,
    pub source: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(default)]
pub struct UnknownInline {
    pub kind: String,
    pub fallback_text: Option<String>,
    pub message: Option<String>,
    pub source: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct ConversionWarning {
    pub code: WarningCode,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WarningCode {
    #[default]
    Unknown,
    UsedHwpxPreviewFallback,
    UnsupportedEquation,
    UnsupportedShape,
    UnsupportedChart,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wraps_paragraphs_into_single_section_document() {
        let document = Document::from_paragraphs(vec!["a".to_string(), "b".to_string()]);

        assert_eq!(document.ir_version, IR_VERSION);
        assert_eq!(document.sections.len(), 1);
        assert_eq!(document.sections[0].blocks.len(), 2);
        assert!(document.resources.entries.is_empty());
        assert!(document.styles.text_styles.is_empty());
        assert!(document.styles.paragraph_styles.is_empty());
        assert!(document.notes.notes.is_empty());

        match &document.sections[0].blocks[0] {
            Block::Paragraph(paragraph) => {
                assert_eq!(paragraph.role, ParagraphRole::Body);
                assert_eq!(paragraph.inlines.len(), 1);
                assert_eq!(paragraph.style, ParagraphStyle::default());
                assert_eq!(paragraph.style_ref, None);
                assert_eq!(paragraph.list, None);
                assert_eq!(
                    paragraph.inlines[0],
                    Inline::Text(TextRun {
                        text: "a".to_string(),
                        style: TextStyle::default(),
                        style_ref: None,
                    })
                );
            }
            other => panic!("expected paragraph block, got {other:?}"),
        }

        match &document.sections[0].blocks[1] {
            Block::Paragraph(paragraph) => {
                assert_eq!(
                    paragraph.inlines[0],
                    Inline::Text(TextRun {
                        text: "b".to_string(),
                        style: TextStyle::default(),
                        style_ref: None,
                    })
                );
            }
            other => panic!("expected paragraph block, got {other:?}"),
        }
    }

    #[test]
    fn paragraph_fallback_preserves_line_breaks_and_tabs_as_inlines() {
        let document = Document::from_paragraphs(vec!["a\nb\tc".to_string()]);

        match &document.sections[0].blocks[0] {
            Block::Paragraph(paragraph) => {
                assert_eq!(
                    paragraph.inlines,
                    vec![
                        Inline::Text(TextRun {
                            text: "a".to_string(),
                            style: TextStyle::default(),
                            style_ref: None,
                        }),
                        Inline::LineBreak,
                        Inline::Text(TextRun {
                            text: "b".to_string(),
                            style: TextStyle::default(),
                            style_ref: None,
                        }),
                        Inline::Tab,
                        Inline::Text(TextRun {
                            text: "c".to_string(),
                            style: TextStyle::default(),
                            style_ref: None,
                        }),
                    ]
                );
            }
            other => panic!("expected paragraph block, got {other:?}"),
        }
    }

    #[test]
    fn document_has_default_resource_store() {
        let document = Document::default();

        assert!(document.resources.entries.is_empty());
    }

    #[test]
    fn document_has_default_style_sheet() {
        let document = Document::default();

        assert!(document.styles.text_styles.is_empty());
        assert!(document.styles.paragraph_styles.is_empty());
        assert!(document.styles.table_styles.is_empty());
        assert!(document.styles.table_cell_styles.is_empty());
    }

    #[test]
    fn document_has_default_note_store() {
        let document = Document::default();

        assert!(document.notes.notes.is_empty());
    }

    #[test]
    fn deserializes_table_cell_without_new_fields() {
        let cell: TableCell =
            serde_json::from_str(r#"{ "row_span": 1, "col_span": 1, "blocks": [], "style": {} }"#)
                .expect("older table cell JSON should deserialize");

        assert!(!cell.is_header);
        assert_eq!(cell.style.vertical_align, None);
        assert_eq!(cell.style.width, None);
        assert_eq!(cell.style.height, None);
        assert_eq!(cell.style.padding_top, None);
        assert_eq!(cell.style.padding_left, None);
        assert_eq!(cell.style.border_top, None);
        assert_eq!(cell.style.border_left, None);
    }

    #[test]
    fn table_cell_can_hold_nested_blocks() {
        let cell = TableCell {
            blocks: vec![Block::Paragraph(Paragraph {
                role: ParagraphRole::Body,
                inlines: vec![Inline::Text(TextRun {
                    text: "cell paragraph".to_string(),
                    style: TextStyle::default(),
                    style_ref: None,
                })],
                style: ParagraphStyle::default(),
                style_ref: None,
                list: None,
            })],
            ..Default::default()
        };

        let table = Table {
            rows: vec![TableRow {
                cells: vec![cell],
                height: None,
            }],
            style: TableStyle::default(),
        };

        match &table.rows[0].cells[0].blocks[0] {
            Block::Paragraph(paragraph) => {
                assert_eq!(paragraph.inlines.len(), 1);
            }
            other => panic!("expected paragraph block, got {other:?}"),
        }
    }

    #[test]
    fn deserializes_older_image_without_new_fields() {
        let image: Image = serde_json::from_str(
            r#"{ "resource_id": "image-1", "alt": null, "caption": null, "width": null, "height": null }"#,
        )
        .expect("older image JSON should deserialize");

        assert_eq!(image.border, None);
        assert!(!image.grayscale);
        assert_eq!(image.effect, None);
        assert_eq!(image.placement, None);
        assert_eq!(image.crop, None);
        assert_eq!(image.brightness, None);
        assert_eq!(image.contrast, None);
        assert_eq!(image.opacity, None);
    }

    #[test]
    fn image_can_reference_image_resource_by_resource_id() {
        let resource_id = ResourceId("image-1".to_string());
        let image = Image {
            resource_id: resource_id.clone(),
            alt: Some("logo".to_string()),
            caption: None,
            width: Some(LengthPx(128.0)),
            height: Some(LengthPx(64.0)),
            ..Default::default()
        };
        let store = ResourceStore {
            entries: vec![Resource::Image(ImageResource {
                id: resource_id.clone(),
                media_type: Some("image/png".to_string()),
                extension: Some("png".to_string()),
                bytes: vec![137, 80, 78, 71],
            })],
        };

        match store.get(&image.resource_id) {
            Some(Resource::Image(resource)) => {
                assert_eq!(resource.id, resource_id);
                assert_eq!(resource.extension.as_deref(), Some("png"));
            }
            other => panic!("expected image resource, got {other:?}"),
        }
    }

    #[test]
    fn equation_serializes_and_deserializes() {
        let equation = Equation {
            kind: EquationKind::Latex,
            content: Some("E = mc^2".to_string()),
            fallback_text: Some("E = mc^2".to_string()),
            resource_id: Some(ResourceId("equation-1".to_string())),
            ..Default::default()
        };

        let json = serde_json::to_string(&equation).expect("equation should serialize");
        let restored: Equation = serde_json::from_str(&json).expect("equation should deserialize");

        assert_eq!(restored, equation);
    }

    #[test]
    fn shape_serializes_and_deserializes() {
        let shape = Shape {
            kind: ShapeKind::Rectangle,
            fallback_text: Some("boxed note".to_string()),
            description: Some("callout".to_string()),
            border: Some(Border {
                width: LengthPx(1.0),
                style: BorderStyle::Solid,
                color: Some(Color {
                    r: 17,
                    g: 34,
                    b: 51,
                    a: 255,
                }),
            }),
            background_color: Some(Color {
                r: 68,
                g: 85,
                b: 102,
                a: 255,
            }),
            rotation_degrees: Some(90.0),
            flip_horizontal: Some(true),
            flip_vertical: Some(true),
            text_vertical_align: Some(VerticalAlign::Middle),
            padding_top: Some(LengthPx(1.0)),
            padding_right: Some(LengthPx(2.0)),
            padding_bottom: Some(LengthPx(3.0)),
            padding_left: Some(LengthPx(4.0)),
            ..Default::default()
        };

        let json = serde_json::to_string(&shape).expect("shape should serialize");
        let restored: Shape = serde_json::from_str(&json).expect("shape should deserialize");

        assert_eq!(restored, shape);
    }

    #[test]
    fn chart_serializes_and_deserializes() {
        let chart = Chart {
            title: Some("Quarterly Sales".to_string()),
            fallback_text: Some("sales chart".to_string()),
            resource_id: Some(ResourceId("chart-1".to_string())),
        };

        let json = serde_json::to_string(&chart).expect("chart should serialize");
        let restored: Chart = serde_json::from_str(&json).expect("chart should deserialize");

        assert_eq!(restored, chart);
    }

    #[test]
    fn rejects_duplicate_resource_ids() {
        let resource_id = ResourceId("image-1".to_string());
        let mut store = ResourceStore::default();

        store
            .insert_unique(Resource::Image(ImageResource {
                id: resource_id.clone(),
                media_type: Some("image/png".to_string()),
                extension: Some("png".to_string()),
                bytes: vec![1, 2, 3],
            }))
            .expect("first insert should succeed");

        let error = store
            .insert_unique(Resource::Binary(BinaryResource {
                id: resource_id.clone(),
                media_type: Some("application/octet-stream".to_string()),
                extension: Some("bin".to_string()),
                bytes: vec![4, 5, 6],
            }))
            .expect_err("duplicate insert should fail");

        assert_eq!(error.resource_id, resource_id);
        assert_eq!(store.entries.len(), 1);
    }

    #[test]
    fn deserializes_older_document_without_resources_and_warnings() {
        let json = r#"{
            "ir_version": 5,
            "metadata": {},
            "sections": []
        }"#;

        let document: Document =
            serde_json::from_str(json).expect("older JSON should still deserialize");

        assert!(document.resources.entries.is_empty());
        assert!(document.styles.text_styles.is_empty());
        assert!(document.notes.notes.is_empty());
        assert!(document.warnings.is_empty());
    }

    #[test]
    fn deserializes_older_text_style_without_new_fields() {
        let style: TextStyle = serde_json::from_str(
            r#"{
                "bold": true,
                "font_family": "Noto Sans KR"
            }"#,
        )
        .expect("older text style JSON should deserialize");

        assert!(style.bold);
        assert!(!style.italic);
        assert!(!style.underline);
        assert!(!style.strike);
        assert!(!style.superscript);
        assert!(!style.subscript);
        assert!(!style.emphasis_dot);
        assert!(!style.emboss);
        assert!(!style.engrave);
        assert!(!style.outline);
        assert!(!style.shadow);
        assert_eq!(style.underline_color, None);
        assert_eq!(style.strike_color, None);
        assert_eq!(style.underline_style, None);
        assert_eq!(style.strike_style, None);
        assert!(!style.underline_above);
        assert_eq!(style.font_family.as_deref(), Some("Noto Sans KR"));
        assert_eq!(style.font_size_pt, None);
        assert_eq!(style.color, None);
        assert_eq!(style.background_color, None);
        assert_eq!(style.font_width_percent, None);
        assert_eq!(style.letter_spacing_percent, None);
        assert_eq!(style.relative_size_percent, None);
        assert_eq!(style.vertical_offset_percent, None);
        assert!(!style.kerning);
    }

    #[test]
    fn text_style_typographic_metrics_round_trip() {
        let style = TextStyle {
            underline: true,
            strike: true,
            underline_style: Some(TextDecorationStyle::Wavy),
            strike_style: Some(TextDecorationStyle::Double),
            underline_above: true,
            font_width_percent: Some(Percent(95.0)),
            letter_spacing_percent: Some(Percent(-5.0)),
            relative_size_percent: Some(Percent(80.0)),
            vertical_offset_percent: Some(Percent(10.0)),
            kerning: true,
            ..Default::default()
        };

        let json = serde_json::to_string(&style).expect("text style should serialize");
        let restored: TextStyle =
            serde_json::from_str(&json).expect("text style should deserialize");

        assert_eq!(restored, style);
        assert!(json.contains("\"font_width_percent\":95.0"));
        assert!(json.contains("\"letter_spacing_percent\":-5.0"));
        assert!(json.contains("\"kerning\":true"));
        assert!(json.contains("\"underline_style\":\"wavy\""));
        assert!(json.contains("\"underline_above\":true"));
    }

    #[test]
    fn paragraph_border_style_round_trip() {
        let style = ParagraphStyle {
            background_color: Some(Color {
                r: 17,
                g: 34,
                b: 51,
                a: 255,
            }),
            padding_left_pt: Some(LengthPt(2.0)),
            border_left: Some(Border {
                width: LengthPx(1.0),
                style: BorderStyle::Dotted,
                color: Some(Color {
                    r: 68,
                    g: 85,
                    b: 102,
                    a: 255,
                }),
            }),
            widow_orphan: true,
            keep_with_next: true,
            keep_lines: true,
            page_break_before: true,
            ..Default::default()
        };

        let json = serde_json::to_string(&style).expect("paragraph style should serialize");
        let restored: ParagraphStyle =
            serde_json::from_str(&json).expect("paragraph style should deserialize");

        assert_eq!(restored, style);
        assert!(json.contains("\"background_color\""));
        assert!(json.contains("\"padding_left_pt\":2.0"));
        assert!(json.contains("\"border_left\""));
        assert!(json.contains("\"keep_with_next\":true"));
        assert!(json.contains("\"page_break_before\":true"));
        assert!(!json.contains("\"border_right\""));
    }

    #[test]
    fn deserializes_legacy_font_size_field_into_points() {
        let style: TextStyle = serde_json::from_str(
            r#"{
                "font_size": 11.5
            }"#,
        )
        .expect("legacy font_size field should deserialize");

        assert_eq!(style.font_size_pt, Some(LengthPt(11.5)));
    }

    #[test]
    fn text_run_style_ref_defaults_to_none() {
        let run: TextRun = serde_json::from_str(
            r#"{
                "text": "styled text",
                "style": {
                    "bold": true
                }
            }"#,
        )
        .expect("text run without style_ref should deserialize");

        assert_eq!(run.style_ref, None);
        assert!(run.style.bold);
    }

    #[test]
    fn paragraph_style_defaults_when_missing_from_json() {
        let paragraph: Paragraph = serde_json::from_str(
            r#"{
                "role": {
                    "role": "body"
                },
                "inlines": [
                    {
                        "type": "text",
                        "text": "paragraph"
                    }
                ]
            }"#,
        )
        .expect("paragraph without style fields should deserialize");

        assert_eq!(paragraph.style, ParagraphStyle::default());
        assert_eq!(paragraph.style_ref, None);
        assert_eq!(paragraph.list, None);
    }

    #[test]
    fn color_alpha_defaults_to_opaque() {
        let color: Color = serde_json::from_str(
            r#"{
                "r": 12,
                "g": 34,
                "b": 56
            }"#,
        )
        .expect("color without alpha should deserialize");

        assert_eq!(
            color,
            Color {
                r: 12,
                g: 34,
                b: 56,
                a: 255,
            }
        );
    }

    #[test]
    fn length_units_deserialize_as_plain_numbers() {
        let millimeters: LengthMm =
            serde_json::from_str("210.0").expect("millimeter value should deserialize");

        assert_eq!(millimeters, LengthMm(210.0));
    }

    #[test]
    fn section_defaults_headers_and_footers_when_missing_from_json() {
        let section: Section = serde_json::from_str(
            r#"{
                "blocks": []
            }"#,
        )
        .expect("section without headers and footers should deserialize");

        assert!(section.headers.is_empty());
        assert!(section.footers.is_empty());
    }

    #[test]
    fn rejects_duplicate_note_ids() {
        let note_id = NoteId("note-1".to_string());
        let mut store = NoteStore::default();

        store
            .insert_unique(Note {
                id: note_id.clone(),
                kind: NoteKind::Footnote,
                blocks: vec![],
            })
            .expect("first note insert should succeed");

        let error = store
            .insert_unique(Note {
                id: note_id.clone(),
                kind: NoteKind::Endnote,
                blocks: vec![],
            })
            .expect_err("duplicate note insert should fail");

        assert_eq!(error.note_id, note_id);
        assert_eq!(store.notes.len(), 1);
    }

    #[test]
    fn unknown_block_defaults_new_fields_when_missing() {
        let unknown: UnknownBlock = serde_json::from_str(
            r#"{
                "kind": "opaque_block",
                "fallback_text": "fallback"
            }"#,
        )
        .expect("older unknown block JSON should deserialize");

        assert_eq!(unknown.kind, "opaque_block");
        assert_eq!(unknown.fallback_text.as_deref(), Some("fallback"));
        assert_eq!(unknown.message, None);
        assert_eq!(unknown.source, None);
    }

    #[test]
    fn unknown_inline_defaults_new_fields_when_missing() {
        let unknown: UnknownInline = serde_json::from_str(
            r#"{
                "kind": "opaque_inline",
                "fallback_text": "fallback"
            }"#,
        )
        .expect("older unknown inline JSON should deserialize");

        assert_eq!(unknown.kind, "opaque_inline");
        assert_eq!(unknown.fallback_text.as_deref(), Some("fallback"));
        assert_eq!(unknown.message, None);
        assert_eq!(unknown.source, None);
    }
}
