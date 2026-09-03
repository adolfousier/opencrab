//! The JSON schema `generate_document` advertises to the model: one
//! object with the per-format sections (`sheets`, `blocks`, `slides`) and
//! the optional `style` block.

use serde_json::Value;

/// The `generate_document` input schema.
pub(super) fn input_schema() -> Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "format": {
                "type": "string",
                "enum": ["xlsx", "docx", "pdf", "pptx"],
                "description": "Output format. \"xlsx\" creates an Excel workbook, \"docx\" a Word document, \"pdf\" a PDF, \"pptx\" a PowerPoint deck (requires python-pptx on the host)."
            },
            "output": {
                "type": "string",
                "description": "Path to write the generated file to (extension should match the format)."
            },
            "sheets": {
                "type": "array",
                "description": "Worksheets to create (xlsx). At least one required.",
                "items": {
                    "type": "object",
                    "properties": {
                        "name": {
                            "type": "string",
                            "description": "Worksheet name (max 31 chars, Excel limit)."
                        },
                        "rows": {
                            "type": "array",
                            "description": "Rows of cells. A cell is a string, number, or boolean. Strings starting with \"=\" become live Excel formulas.",
                            "items": {"type": "array"}
                        },
                        "header_bold": {
                            "type": "boolean",
                            "description": "Bold the first row as a header (default: true)."
                        },
                        "column_widths": {
                            "type": "array",
                            "items": {"type": "number"},
                            "description": "Optional column widths, in Excel character units, applied left to right."
                        }
                    },
                    "required": ["name", "rows"]
                }
            },
            "title": {
                "type": "string",
                "description": "PDF metadata title (pdf only; defaults to the output file stem)."
            },
            "blocks": {
                "type": "array",
                "description": "Content blocks in order (docx/pdf). At least one required.",
                "items": {
                    "type": "object",
                    "properties": {
                        "type": {
                            "type": "string",
                            "enum": ["heading", "paragraph", "list", "table"],
                            "description": "Block kind."
                        },
                        "text": {"type": "string", "description": "Text for heading/paragraph blocks."},
                        "level": {"type": "integer", "minimum": 1, "maximum": 3, "description": "Heading level (default 1)."},
                        "bold": {"type": "boolean", "description": "Bold the paragraph text."},
                        "items": {"type": "array", "items": {"type": "string"}, "description": "List items."},
                        "ordered": {"type": "boolean", "description": "Numbered list instead of bullets (default false)."},
                        "rows": {"type": "array", "items": {"type": "array"}, "description": "Table rows of cells."},
                        "path": {"type": "string", "description": "Local PNG/JPEG path for image blocks."},
                        "width_mm": {"type": "number", "description": "Rendered image width in millimeters (aspect preserved; clamped to the page body). Default: natural size, downscaled to fit."},
                        "caption": {"type": "string", "description": "Caption text under an image block."},
                        "header_bold": {"type": "boolean", "description": "Bold the first table row (default true)."}
                    },
                    "required": ["type"]
                }
            },
            "style": {
                "type": "object",
                "description": "Visual styling, interpreted per format. All fields optional. PDF: accent_color, text_color, page_header{text,logo_path}, page_footer{text,page_numbers}, zebra_rows, orientation (portrait/landscape, default portrait), page_size{width_mm,height_mm} (custom dimensions; takes precedence over orientation). XLSX: header_fill, header_font_color, zebra_rows, freeze_header, autofilter, tab_color (hex colors); per-sheet column_formats on each sheet. DOCX: accent_color (heading color), page_header, page_footer, table_header_fill, zebra_rows, orientation (portrait/landscape), page_size{width_mm,height_mm}. PPTX: template_path (existing .pptx whose master/branding the slides inherit; best branding lever), accent_color (title color), slide_size (preset \"16:9\"/\"4:3\"/\"square\" or {width_inches,height_inches}; ignored with template_path), per-slide layout index. Use when the user wants polished or branded output.",
                "properties": {
                    "accent_color": {"type": "string", "description": "Hex color (\"#0A84FF\") for headings, H1 underline bar, and table header separator."},
                    "text_color": {"type": "string", "description": "Hex color for body text (default near-black)."},
                    "page_header": {
                        "type": "object",
                        "properties": {
                            "text": {"type": "string", "description": "Brand/report name shown at the top of every page with an accent rule."},
                            "logo_path": {"type": "string", "description": "Local PNG/JPEG path drawn in the page header band."}
                        }
                    },
                    "page_footer": {
                        "type": "object",
                        "properties": {
                            "text": {"type": "string", "description": "Footer text on every page."},
                            "page_numbers": {"type": "boolean", "description": "Render \"Page N of M\" bottom-right."}
                        }
                    },
                    "zebra_rows": {"type": "boolean", "description": "Alternating light fills behind table rows."},
                    "orientation": {
                        "type": "string",
                        "enum": ["portrait", "landscape"],
                        "description": "Page orientation for PDF output (default portrait). Ignored when page_size is set."
                    },
                    "page_size": {
                        "type": "object",
                        "description": "Custom page dimensions in mm for PDF output (takes precedence over orientation).",
                        "properties": {
                            "width_mm": {"type": "number", "minimum": 100, "description": "Page width in millimeters."},
                            "height_mm": {"type": "number", "minimum": 100, "description": "Page height in millimeters."}
                        },
                        "required": ["width_mm", "height_mm"]
                    }
                }
            },
            "slides": {
                "type": "array",
                "description": "Slides to create (pptx). At least one required.",
                "items": {
                    "type": "object",
                    "properties": {
                        "title": {"type": "string", "description": "Slide title."},
                        "bullets": {"type": "array", "items": {"type": "string"}, "description": "Bullet lines for the slide body."},
                        "notes": {"type": "string", "description": "Optional speaker notes."}
                    },
                    "required": ["title"]
                }
            }
        },
        "required": ["format", "output"]
    })
}
