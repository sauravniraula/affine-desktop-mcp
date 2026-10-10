# Independent Yjs fixtures

These binary updates were generated with JavaScript `yjs` 13.6.33. They are
synthetic test data, not copies of personal AFFiNE documents.

- `root.bin`: `meta.name = "Fixture Workspace"`; active `fixture-page` and
  trashed `fixture-trash` entries in the `meta.pages` Y.Array.
- `page.bin`: a page root, note with paragraph and flattened table, and a surface
  with wrapped canvas text. Title: `Fixture Page`; paragraph: `Snapshot `.
- `page-update.bin`: a dependent incremental update appending `plus update 🌏`
  to the paragraph. Applying it without the snapshot must be rejected as
  incomplete by the bridge.
- `native-canvas.bin`: an independent page + surface with native ellipse/diamond
  shapes and a bound, labeled arrow; labels are Y.Text values, not paragraphs.
  Regenerate with `generate-native-canvas.mjs` and JavaScript `yjs` installed in a
  scratch directory (`YJS_MODULE=/absolute/path/to/yjs/dist/yjs.mjs node
  tests/fixtures/generate-native-canvas.mjs`). Node is only needed to regenerate
  fixtures, not to run the Rust server or its tests.

- `native-drawing.bin`: independent native brush with pressure points, unbound
  curved connector with Y.Text label, frame with Y.Text title and flat child
  membership/background fields, external/internal link cards, image and
  attachment blocks. Regenerate with `generate-native-drawing.mjs`, using the
  same `YJS_MODULE` override. The script also generates `sample-media.png`
  (a real 8x8 RGB PNG with computed CRCs) and `sample-attachment.txt`; their
  SHA-256/base64url IDs match the native media references. All are synthetic.

They verify that the Rust Yrs decoder reads independently produced Yjs data,
including native surface wrappers, rich-text values and pending updates.
