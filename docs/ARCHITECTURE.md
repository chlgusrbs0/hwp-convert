# hwp-convert Architecture

This document fixes the current architecture boundaries for the first Document IR roadmap pass.

The project now has two separate output paths:

1. The semantic conversion path for `txt`, `json`, `markdown`, `html`, and the existing semantic `svg`.
2. The experimental renderer-first visual path for render inspection and diagnostics.

## Layer Boundaries

### 1. `rhwp` parser and renderer

`rhwp` is the source of truth for reading `.hwp` and `.hwpx` files.

- Parser role: read document structure, text, styles, controls, and embedded resources.
- Renderer role: expose page and layout-oriented query results for visual inspection.
- Boundary: `rhwp` types stay behind `bridge` and `render`. Exporters must not depend on `rhwp` types directly.

### 2. `bridge`

`src/bridge` converts parsed document data into the semantic `Document` IR.

- Responsibility: semantic mapping only.
- It maps paragraphs, tables, images, notes, headers, footers, links, lists, equations, shapes, charts, and unknown elements into stable IR nodes.
- It may improve element coverage and mapping quality without changing exporter contracts.
- Bridge-only changes do not require an `IR_VERSION` bump.

### 3. Semantic `Document` IR

`src/ir` defines the stable semantic document model used by the existing exporters.

- Responsibility: preserve document meaning and structure in a format-independent way.
- It is not a layout engine.
- Page coordinates and render-only geometry must not be mixed into this layer.
- Unknown or unsupported elements should be preserved through `Unknown` nodes or `ConversionWarning` records instead of being silently dropped.

### 4. Exporters

`src/exporter.rs` renders the semantic `Document` IR into output formats.

- Supported path today: `txt`, `json`, `markdown`, `html`, and the existing semantic `svg`.
- Boundary: exporters only consume the semantic IR and related assets exposed by the project.
- Exporters must not read `rhwp` parser or renderer types directly.
- The current `--to svg` behavior remains the semantic exporter path and is not replaced by renderer experiments.

Asset output policy today is intentionally narrow: HTML and Markdown exports write `Resource::Image` bytes under a document-scoped asset directory based on the output file stem. For example, `out/sample.html` and `out/sample.md` reference `sample_assets/images/image-1.png`, with bytes written to `out/sample_assets/images/image-1.png`. This replaces the older shared sibling `images/` directory policy so multiple output documents in one directory do not collide on names like `image-1.png`. The file name still comes from the shared resource file-name rule. TXT, JSON, semantic SVG, and RenderSnapshot diagnostics do not share this asset writer.

### 5. `RenderSnapshot` and the visual path

`src/render` contains the renderer-first experimental path.

- Responsibility: inspect renderer query output without redefining the semantic IR.
- `RenderSnapshot` captures page, item, and bounds information for diagnostics.
- `RenderSnapshotSummary` aggregates page and item counts for quick inspection.
- Visual SVG helpers render placeholder-oriented page previews from renderer coordinates.
- This SVG path is experimental. It uses renderer query coordinates as-is and does not define a final layout schema.

## Current State

- The semantic `Document` IR is in a first-complete phase for the current roadmap.
- `RenderSnapshot` is an experimental visual path kept separate from the semantic IR.
- The existing SVG exporter is still the semantic/plain-text-oriented exporter path.
- PDF output is not implemented yet.
- Local `sample.hwp` and `sample.hwpx` files are for developer verification only and must not be committed.
- Renderer-first artifacts such as visual checks belong under `target/render-check/` or other local-only output directories.

## Document IR Roadmap

### v0

- Introduce the minimum `Document` IR.
- Establish a stable semantic conversion target instead of exporter-specific ad hoc output logic.

### v1

- Move exporters onto the shared IR.
- Make `txt`, `json`, `markdown`, and `html` flow through the same semantic document model.

### v2

- Add `Table` IR.
- Preserve table structure as first-class document content instead of flattening tables into plain text.

### v3

- Add image support and `ResourceStore`.
- Separate document content from reusable binary resources and asset references.

### v3.1

- Stabilize the IR after the first image/resource iteration.
- Tighten contracts before adding broader element coverage.

### v4

- Add style-oriented IR support.
- Preserve semantic styling information without turning the IR into a presentation-specific layout model.

### v5

- Add note, header, footer, link, and list coverage.
- Extend the semantic IR to handle more real document structure.

### v6

- Add equation, shape, chart, and unknown-element coverage.
- Preserve more content classes and explicitly carry forward unsupported cases.

### v6.5

- Improve table and image bridge mapping.
- Raise semantic fidelity without redesigning the IR.

### v6.6

- Improve bridge mapping for the v5 element set.
- Focus on notes, headers, footers, links, and lists.

### v7.0

- Start renderer-first investigation.
- Add experimental `RenderSnapshot` support through public `rhwp` renderer query APIs.

### v7.1

- Add `RenderSnapshotSummary`.
- Provide quick page and item counting for renderer output inspection.

### v7.2

- Add `RenderSnapshot -> SVG` experimental output.
- Visualize page bounds, text, and control placeholders without replacing the semantic SVG exporter.

### v7.3

- Add visual check artifact output for local samples.
- Support writing renderer-first inspection files under `target/render-check/`.

### v7.4

- Freeze the first-pass architecture boundaries.
- Shift focus from growing the IR surface area to improving bridge, exporter, and render quality.

## Development Principles

1. Exporters do not depend directly on `rhwp` types.
2. Bridge-only changes do not bump `IR_VERSION`.
3. `RenderSnapshot` must not inject coordinates into the semantic `Document` IR.
4. Visual output is handled in the `RenderSnapshot` layer, not by mutating the semantic IR.
5. Unknown elements are preserved through `Unknown` nodes or `ConversionWarning` records whenever possible.
6. Public `rhwp` APIs are preferred. Private renderer internals are out of scope.
7. Renderer experiments must not change the existing CLI contract or replace the semantic exporter path by accident.

## Limits and TODO

- A real fixture corpus is still needed for broader regression coverage.
- Anchored controls and read-order fidelity still need work.
- Renderer-first image output is still placeholder-based. Image bytes are not embedded into SVG.
- Renderer-first table output is still placeholder-oriented and does not implement full table visual fidelity.
- PDF output is a separate future stage.
- Asset bundle policy beyond HTML/Markdown image files and renderer diagnostics still needs a dedicated design pass.

## Practical Guidance

- Use `src/bridge` when semantic mapping quality needs improvement.
- Use `src/exporter.rs` when output formatting of the semantic IR needs improvement.
- Use `src/render` when renderer-first inspection, page diagnostics, or visual placeholder output needs improvement.
- Do not grow the semantic IR further unless a new semantic gap is clearly identified.
