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
/// v35: added `BinaryResource` kind and source path metadata.
/// v36: resource bytes serialize as Base64 strings. Deserialization still
/// accepts the legacy JSON byte-array representation.
/// v37: generalized image placement as `ObjectPlacement` and added optional
/// table placement metadata.
/// v38: added resolved custom tab definitions to `ParagraphStyle`.
/// v39: added structured shape geometry and object placement metadata.
/// v40: added script-specific variants to named text styles.
/// v41: added structured section and page layout metadata.
/// v42: added ordered column layout change blocks.
/// v43: added structured page and numbering control blocks.
/// v44: added structured document field inlines.
/// v45: added structured table cell fill styles.
/// v46: added table cell source coordinates and border-fill zones.
/// v47: added structured shape fill styles.
/// v48: added structured shape groups and nested child blocks.
/// v49: added structured shape text-box content blocks.
pub const IR_VERSION: u16 = 49;

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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub layout: Option<SectionLayout>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SectionLayout {
    pub raw_flags: u32,
    pub column_spacing: LengthPx,
    pub default_tab_spacing: LengthPx,
    pub page_number_start: u16,
    pub page_number_type: u8,
    pub picture_number_start: u16,
    pub table_number_start: u16,
    pub equation_number_start: u16,
    pub outline_numbering_id: u16,
    pub text_direction: u8,
    pub hide_header: bool,
    pub hide_footer: bool,
    pub hide_master_page: bool,
    pub hide_border: bool,
    pub hide_fill: bool,
    pub hide_empty_line: bool,
    pub page: PageLayout,
    pub footnote: NoteLayout,
    pub endnote: NoteLayout,
    pub page_border_fills: Vec<PageBorderFillLayout>,
    #[serde(default, with = "base64_bytes", skip_serializing_if = "Vec::is_empty")]
    pub raw_control_extension: Vec<u8>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub extra_records: Vec<RawSectionRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PageLayout {
    pub width: LengthPx,
    pub height: LengthPx,
    pub margin_left: LengthPx,
    pub margin_right: LengthPx,
    pub margin_top: LengthPx,
    pub margin_bottom: LengthPx,
    pub margin_header: LengthPx,
    pub margin_footer: LengthPx,
    pub margin_gutter: LengthPx,
    pub raw_attributes: u32,
    pub landscape: bool,
    pub binding: PageBinding,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PageBinding {
    SingleSided,
    DuplexSided,
    TopFlip,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct NoteLayout {
    pub raw_attributes: u32,
    pub number_format: String,
    pub user_character: Option<char>,
    pub prefix_character: Option<char>,
    pub suffix_character: Option<char>,
    pub start_number: u16,
    pub separator_length: LengthPx,
    pub separator_margin_top: LengthPx,
    pub separator_margin_bottom: LengthPx,
    pub note_spacing: LengthPx,
    pub separator_line_type: u8,
    pub separator_line_width: u8,
    pub separator_color_raw: u32,
    pub numbering: String,
    pub placement: String,
    pub raw_unknown: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PageBorderFillLayout {
    pub raw_attributes: u32,
    pub spacing_left: LengthPx,
    pub spacing_right: LengthPx,
    pub spacing_top: LengthPx,
    pub spacing_bottom: LengthPx,
    pub border_fill_id: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RawSectionRecord {
    pub tag_id: u16,
    pub level: u16,
    #[serde(with = "base64_bytes")]
    pub data: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Block {
    Paragraph(Paragraph),
    ColumnLayout(ColumnLayout),
    DocumentControl(DocumentControl),
    Table(Table),
    Image(Image),
    Equation(Equation),
    Shape(Shape),
    Chart(Chart),
    Unknown(UnknownBlock),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "control", rename_all = "snake_case")]
pub enum DocumentControl {
    AutoNumber {
        kind: NumberingKind,
        number: u16,
        assigned_number: u16,
        format: u8,
        superscript: bool,
        user_symbol: Option<char>,
        prefix: Option<char>,
        suffix: Option<char>,
        fallback_text: String,
    },
    NewNumber {
        kind: NumberingKind,
        number: u16,
        fallback_text: String,
    },
    PageNumberPosition {
        format: u8,
        position: u8,
        user_symbol: Option<char>,
        prefix: Option<char>,
        suffix: Option<char>,
        dash: Option<char>,
        fallback_text: String,
    },
    PageVisibility {
        hide_header: bool,
        hide_footer: bool,
        hide_master_page: bool,
        hide_border: bool,
        hide_fill: bool,
        hide_page_number: bool,
        fallback_text: String,
    },
}

impl DocumentControl {
    pub fn fallback_text(&self) -> &str {
        match self {
            Self::AutoNumber { fallback_text, .. }
            | Self::NewNumber { fallback_text, .. }
            | Self::PageNumberPosition { fallback_text, .. }
            | Self::PageVisibility { fallback_text, .. } => fallback_text,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum NumberingKind {
    Page,
    Footnote,
    Endnote,
    Picture,
    Table,
    Equation,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ColumnLayout {
    pub kind: ColumnLayoutKind,
    pub column_count: u16,
    pub direction: ColumnDirection,
    pub same_width: bool,
    pub spacing: LengthPx,
    pub raw_widths: Vec<i16>,
    pub raw_gaps: Vec<i16>,
    pub proportional_widths: bool,
    pub separator_type: u8,
    pub separator_width: u8,
    pub separator_color_raw: u32,
    pub raw_attributes: u16,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ColumnLayoutKind {
    Normal,
    Distribute,
    Parallel,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ColumnDirection {
    LeftToRight,
    RightToLeft,
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
    Field(DocumentField),
    FootnoteRef { note_id: NoteId },
    EndnoteRef { note_id: NoteId },
    Unknown(UnknownInline),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DocumentField {
    pub kind: FieldKind,
    pub command: Option<String>,
    pub properties: u32,
    pub extra_properties: u8,
    pub field_id: u32,
    pub control_id: u32,
    pub control_data_name: Option<String>,
    pub memo_index: u32,
    pub fallback_text: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FieldKind {
    Unknown,
    Date,
    DocumentDate,
    Path,
    Bookmark,
    MailMerge,
    CrossReference,
    Formula,
    ClickHere,
    Hyperlink,
    Summary,
    UserInfo,
    Memo,
    PrivateInfoSecurity,
    TableOfContents,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tab_definition: Option<TabDefinition>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TabDefinition {
    pub source_id: u16,
    pub raw_attributes: u32,
    pub auto_tab_left: bool,
    pub auto_tab_right: bool,
    pub stops: Vec<TabStop>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub struct TabStop {
    pub position_pt: LengthPt,
    pub alignment: TabAlignment,
    pub leader_type: u8,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TabAlignment {
    Left,
    Right,
    Center,
    Decimal,
    Unknown(u8),
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
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub script_styles: Vec<ScriptTextStyle>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ScriptTextStyle {
    pub script: TextScript,
    pub style: TextStyle,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TextScript {
    Korean,
    Latin,
    Hanja,
    Japanese,
    Other,
    Symbol,
    User,
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
    #[serde(with = "base64_bytes")]
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct BinaryResource {
    pub id: ResourceId,
    pub media_type: Option<String>,
    pub extension: Option<String>,
    #[serde(with = "base64_bytes")]
    pub bytes: Vec<u8>,
    #[serde(default)]
    pub kind: BinaryResourceKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub absolute_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub relative_path: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BinaryResourceKind {
    Link,
    #[default]
    Embedded,
    Storage,
    Unknown,
}

mod base64_bytes {
    use base64::Engine as _;
    use base64::engine::general_purpose::STANDARD;
    use serde::{Deserialize, Deserializer, Serializer, de::Error as _};

    #[derive(Deserialize)]
    #[serde(untagged)]
    enum EncodedBytes {
        Base64(String),
        LegacyArray(Vec<u8>),
    }

    pub fn serialize<S>(bytes: &[u8], serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&STANDARD.encode(bytes))
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Vec<u8>, D::Error>
    where
        D: Deserializer<'de>,
    {
        match EncodedBytes::deserialize(deserializer)? {
            EncodedBytes::Base64(encoded) => STANDARD.decode(encoded).map_err(|error| {
                D::Error::custom(format!("invalid Base64 resource bytes: {error}"))
            }),
            EncodedBytes::LegacyArray(bytes) => Ok(bytes),
        }
    }
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub original_width: Option<LengthPx>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub original_height: Option<LengthPx>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_width: Option<LengthPx>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_height: Option<LengthPx>,
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
pub struct ObjectPlacement {
    pub treat_as_character: bool,
    pub flow_with_text: bool,
    pub allow_overlap: bool,
    pub prevent_page_break: bool,
    pub z_order: i32,
    pub text_wrap: ImageTextWrap,
    pub vertical_relative_to: VerticalRelativeTo,
    pub vertical_alignment: VerticalObjectAlignment,
    pub vertical_offset: LengthPx,
    pub horizontal_relative_to: HorizontalRelativeTo,
    pub horizontal_alignment: HorizontalObjectAlignment,
    pub horizontal_offset: LengthPx,
    pub margin_top: LengthPx,
    pub margin_right: LengthPx,
    pub margin_bottom: LengthPx,
    pub margin_left: LengthPx,
}

pub type ImagePlacement = ObjectPlacement;

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
    pub fill: Option<FillStyle>,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub geometry: Option<ShapeGeometry>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub placement: Option<ObjectPlacement>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<Block>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub content: Vec<Block>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ShapeGeometry {
    Line {
        start: ShapePoint,
        end: ShapePoint,
    },
    Rectangle {
        corners: Vec<ShapePoint>,
        round_rate_percent: u8,
    },
    Ellipse {
        center: ShapePoint,
        axis1: ShapePoint,
        axis2: ShapePoint,
    },
    Arc {
        arc_type: u8,
        center: ShapePoint,
        axis1: ShapePoint,
        axis2: ShapePoint,
    },
    Polygon {
        points: Vec<ShapePoint>,
    },
    Curve {
        points: Vec<ShapePoint>,
        segment_types: Vec<u8>,
    },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub struct ShapePoint {
    pub x: LengthPx,
    pub y: LengthPx,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ShapeKind {
    Line,
    Rectangle,
    Ellipse,
    Polygon,
    TextBox,
    Group,
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
    #[serde(default)]
    pub zones: Vec<TableZone>,
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
    #[serde(default)]
    pub source_row: Option<u32>,
    #[serde(default)]
    pub source_column: Option<u32>,
    pub blocks: Vec<Block>,
    pub style: TableCellStyle,
}

impl Default for TableCell {
    fn default() -> Self {
        Self {
            row_span: 1,
            col_span: 1,
            is_header: false,
            source_row: None,
            source_column: None,
            blocks: Vec::new(),
            style: TableCellStyle::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
#[serde(default)]
pub struct TableZone {
    pub start_row: u32,
    pub start_column: u32,
    pub end_row: u32,
    pub end_column: u32,
    pub source_border_fill_id: u16,
    pub fill: Option<FillStyle>,
    pub border_top: Option<Border>,
    pub border_right: Option<Border>,
    pub border_bottom: Option<Border>,
    pub border_left: Option<Border>,
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
    pub placement: Option<ObjectPlacement>,
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
    pub fill: Option<FillStyle>,
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum FillStyle {
    Solid {
        background_color: Option<Color>,
        background_color_raw: u32,
        pattern_color: Option<Color>,
        pattern_color_raw: u32,
        pattern_type: i32,
        alpha: u8,
    },
    Gradient {
        gradient_type: i16,
        angle: i16,
        center_x: i16,
        center_y: i16,
        blur: i16,
        colors: Vec<GradientColor>,
        positions: Vec<i32>,
        alpha: u8,
    },
    Image {
        mode: ImageFillMode,
        brightness: i8,
        contrast: i8,
        effect: u8,
        source_bin_data_id: u16,
        resource_id: Option<ResourceId>,
        alpha: u8,
    },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct GradientColor {
    pub color: Option<Color>,
    pub raw: u32,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ImageFillMode {
    TileAll,
    TileHorizontalTop,
    TileHorizontalBottom,
    TileVerticalLeft,
    TileVerticalRight,
    FitToSize,
    Center,
    CenterTop,
    CenterBottom,
    LeftCenter,
    LeftTop,
    LeftBottom,
    RightCenter,
    RightTop,
    RightBottom,
    None,
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
        assert_eq!(cell.source_row, None);
        assert_eq!(cell.source_column, None);
        assert_eq!(cell.style.fill, None);
        assert_eq!(cell.style.vertical_align, None);
        assert_eq!(cell.style.width, None);
        assert_eq!(cell.style.height, None);
        assert_eq!(cell.style.padding_top, None);
        assert_eq!(cell.style.padding_left, None);
        assert_eq!(cell.style.border_top, None);
        assert_eq!(cell.style.border_left, None);

        let table: Table = serde_json::from_str(r#"{ "rows": [], "style": {} }"#)
            .expect("older table JSON should deserialize");
        assert!(table.zones.is_empty());
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
            ..Default::default()
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
        assert_eq!(image.original_width, None);
        assert_eq!(image.original_height, None);
        assert_eq!(image.current_width, None);
        assert_eq!(image.current_height, None);
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
    fn serializes_resource_bytes_as_base64_strings() {
        let image = ImageResource {
            id: ResourceId("image-1".to_string()),
            media_type: Some("image/png".to_string()),
            extension: Some("png".to_string()),
            bytes: vec![137, 80, 78, 71],
        };
        let binary = BinaryResource {
            id: ResourceId("binary-1".to_string()),
            bytes: vec![1, 2, 3],
            ..Default::default()
        };

        let image_json = serde_json::to_string(&image).expect("image resource should serialize");
        let binary_json = serde_json::to_string(&binary).expect("binary resource should serialize");

        assert!(image_json.contains(r#""bytes":"iVBORw==""#));
        assert!(binary_json.contains(r#""bytes":"AQID""#));
        assert_eq!(
            serde_json::from_str::<ImageResource>(&image_json)
                .expect("Base64 image resource should deserialize"),
            image
        );
        assert_eq!(
            serde_json::from_str::<BinaryResource>(&binary_json)
                .expect("Base64 binary resource should deserialize"),
            binary
        );
    }

    #[test]
    fn deserializes_legacy_resource_byte_arrays() {
        let image: ImageResource = serde_json::from_str(
            r#"{"id":"image-1","media_type":"image/png","extension":"png","bytes":[137,80,78,71]}"#,
        )
        .expect("legacy image byte array should deserialize");
        let binary: BinaryResource = serde_json::from_str(
            r#"{"id":"binary-1","media_type":null,"extension":null,"bytes":[1,2,3]}"#,
        )
        .expect("legacy binary byte array should deserialize");

        assert_eq!(image.bytes, vec![137, 80, 78, 71]);
        assert_eq!(binary.bytes, vec![1, 2, 3]);
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
            geometry: Some(ShapeGeometry::Rectangle {
                corners: vec![
                    ShapePoint {
                        x: LengthPx(0.0),
                        y: LengthPx(0.0),
                    },
                    ShapePoint {
                        x: LengthPx(48.0),
                        y: LengthPx(0.0),
                    },
                    ShapePoint {
                        x: LengthPx(48.0),
                        y: LengthPx(24.0),
                    },
                    ShapePoint {
                        x: LengthPx(0.0),
                        y: LengthPx(24.0),
                    },
                ],
                round_rate_percent: 15,
            }),
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
                ..Default::default()
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
