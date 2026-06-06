# HWPX fixture findings

Last checked: 2026-06-06 KST

This document records observed behavior from the current pinned rHWP revision:

```text
bea635bd708274a51ae3f557a71b07683d7c2454
```

These notes are facts from local fixture experiments, not promises about future rHWP behavior.

## Policy

HWPX paired fixtures are added only when they meet the same feature-level assertions as the matching HWP fixture.

A generated HWPX file is not accepted just because it parses. It must preserve the target semantic structure through:

1. `bridge::rhwp::read_document`
2. `Document IR`
3. fixture feature assertions
4. exporter smoke tests
5. bridge stats expectations

If HWPX output is weaker than HWP for the same fixture, do not commit it as an official paired fixture. Use a real authored HWPX fixture or fix the relevant parser/bridge/exporter path first.

## Accepted paired fixtures

| Fixture | HWP | HWPX | Notes |
| --- | --- | --- | --- |
| `basic_text` | yes | yes | Paragraph text, line break, tab, styled text runs, and bridge stats are preserved. |
| `list` | yes | yes | List paragraph metadata and reading order are preserved. Bullet glyph exactness is still not an assertion target. |

## Rejected synthetic HWPX attempts

The following HWPX files were generated locally from existing HWP fixtures through rHWP parse plus rHWP HWPX serialization, then removed because they did not meet the matching HWP fixture assertions.

| Fixture | Observed failure | Current interpretation |
| --- | --- | --- |
| `table` | Parsed into empty semantic content; `Contents/section0.xml` contained no table content and `Preview/PrvText.txt` was empty. | Current synthetic HWP to HWPX path does not serialize table controls enough to use as a paired fixture. |
| `style` | Text survived, but paragraph spacing expected by the HWP fixture was missing from the bridged HWPX result. The HWPX XML contained paragraph margin values. | Current pinned rHWP HWPX parser/model path appears not to expose all paragraph style data used by the bridge. |
| `equation` | Parsed into empty semantic content with preview fallback warning. | Current synthetic HWPX path is not usable for equation block coverage. |
| `shape` | Parsed into empty semantic content with preview fallback warning. | Current synthetic HWPX path is not usable for shape block coverage. |
| `footnote` | Body content parsed, but the footnote store was not preserved. | Current HWPX path is not usable for note coverage without losing required structure. |
| `header_footer` | Parsed into empty semantic content with preview fallback warning; header/footer assertions failed. | Current synthetic HWPX path is not usable for header/footer coverage. |
| `image` | Parsed into empty semantic content with preview fallback warning; image/resource assertions failed. | Current synthetic HWPX path is not usable for image asset coverage. |

## Practical next steps

1. Continue accepting HWPX paired fixtures only when feature assertions pass at HWP parity.
2. Prefer real authored HWPX fixtures for table, style, image, note, header/footer, equation, and shape coverage.
3. If using rHWP-generated HWPX for another fixture, inspect the generated archive before committing it when tests fail.
4. Treat HWPX preview fallback as text-only recovery. It is useful for user-facing partial conversion, but not enough to claim structural support.
5. Re-run these attempts after an rHWP revision update. Commit dependency update and behavior changes separately.

## Useful verification commands

```bash
cargo test --test fixture_smoke
```

```bash
HWP_CONVERT_UPDATE_FIXTURE_STATS=1 cargo test --test fixture_smoke official_fixtures_match_expected_bridge_stats
```

On PowerShell:

```powershell
$env:HWP_CONVERT_UPDATE_FIXTURE_STATS='1'
cargo test --test fixture_smoke official_fixtures_match_expected_bridge_stats
```
