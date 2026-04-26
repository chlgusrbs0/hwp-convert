use std::error::Error;
use std::path::Path;

use crate::hwpx;
use crate::ir::Document;

pub fn read_document(input_path: &Path) -> Result<Document, Box<dyn Error>> {
    let paragraphs = hwpx::read_paragraphs(input_path)?;
    Ok(Document::from_paragraphs(paragraphs))
}
