use std::error::Error;
use std::fmt;

use serde::{Deserialize, Serialize};

/// Serialized Document IR format version.
///
/// This is independent from the internal roadmap milestones (`v0`-`v7`).
/// Bump this when JSON compatibility changes, such as new enum variants,
/// new required fields, or other output-shape changes.
pub const IR_VERSION: u16 = 4;

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
            warnings: Vec::new(),
        }
    }
}

impl Document {
    pub fn from_paragraphs(paragraphs: Vec<String>) -> Self {
        let blocks = paragraphs
            .into_iter()
            .map(|text| {
                Block::Paragraph(Paragraph {
                    role: ParagraphRole::Body,
                    inlines: vec![Inline::Text(TextRun {
                        text,
                        style: TextStyle::default(),
                        style_ref: None,
                    })],
                    style: ParagraphStyle::default(),
                    style_ref: None,
                })
            })
            .collect();

        Self {
            ir_version: IR_VERSION,
            metadata: Metadata::default(),
            sections: vec![Section { blocks }],
            resources: ResourceStore::default(),
            styles: StyleSheet::default(),
            warnings: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct Metadata {
    pub title: Option<String>,
    pub author: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct Section {
    pub blocks: Vec<Block>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Block {
    Paragraph(Paragraph),
    Table(Table),
    Image(Image),
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
    pub font_family: Option<String>,
    /// Typographic size in points (pt).
    #[serde(alias = "font_size")]
    pub font_size_pt: Option<LengthPt>,
    pub color: Option<Color>,
    pub background_color: Option<Color>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
#[serde(default)]
pub struct ParagraphStyle {
    pub alignment: Option<Alignment>,
    pub spacing: Spacing,
    pub indent: Indent,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Alignment {
    Left,
    Center,
    Right,
    Justify,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
#[serde(default)]
pub struct Spacing {
    pub before_pt: Option<LengthPt>,
    pub after_pt: Option<LengthPt>,
    pub line_pt: Option<LengthPt>,
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
    /// Display hint in px until Layout IR defines document-space units.
    pub width: Option<LengthPx>,
    /// Display hint in px until Layout IR defines document-space units.
    pub height: Option<LengthPx>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct Table {
    pub rows: Vec<TableRow>,
    pub style: TableStyle,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct TableRow {
    pub cells: Vec<TableCell>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TableCell {
    pub row_span: u32,
    pub col_span: u32,
    pub blocks: Vec<Block>,
    pub style: TableCellStyle,
}

impl Default for TableCell {
    fn default() -> Self {
        Self {
            row_span: 1,
            col_span: 1,
            blocks: Vec::new(),
            style: TableCellStyle::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
#[serde(default)]
pub struct TableStyle {
    pub background_color: Option<Color>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
#[serde(default)]
pub struct TableCellStyle {
    pub background_color: Option<Color>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct UnknownBlock {
    pub kind: String,
    pub fallback_text: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct UnknownInline {
    pub kind: String,
    pub fallback_text: Option<String>,
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

        match &document.sections[0].blocks[0] {
            Block::Paragraph(paragraph) => {
                assert_eq!(paragraph.role, ParagraphRole::Body);
                assert_eq!(paragraph.inlines.len(), 1);
                assert_eq!(paragraph.style, ParagraphStyle::default());
                assert_eq!(paragraph.style_ref, None);
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
            })],
            ..Default::default()
        };

        let table = Table {
            rows: vec![TableRow { cells: vec![cell] }],
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
    fn image_can_reference_image_resource_by_resource_id() {
        let resource_id = ResourceId("image-1".to_string());
        let image = Image {
            resource_id: resource_id.clone(),
            alt: Some("logo".to_string()),
            caption: None,
            width: Some(LengthPx(128.0)),
            height: Some(LengthPx(64.0)),
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
            "ir_version": 4,
            "metadata": {},
            "sections": []
        }"#;

        let document: Document =
            serde_json::from_str(json).expect("older JSON should still deserialize");

        assert!(document.resources.entries.is_empty());
        assert!(document.styles.text_styles.is_empty());
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
        assert_eq!(style.font_family.as_deref(), Some("Noto Sans KR"));
        assert_eq!(style.font_size_pt, None);
        assert_eq!(style.color, None);
        assert_eq!(style.background_color, None);
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
}
