# Fixture 계획과 관리 (Fixtures)

이 문서는 HWP/HWPX bridge coverage를 검증하기 위한 fixture의 계획, 관리 규칙, 검증 방법을 한곳에 정리한다. 목표는 새 기능을 먼저 만드는 것이 아니라, 현재 동작과 한계를 문서화하고 회귀 테스트 기반을 만드는 것이다.

현재 채택된 fixture 목록과 HWPX 쌍 현황은 `docs/STATUS.md`의 "HWPX fixture 현황"을 본다. fixture 입력 파일 인벤토리는 `AGENTS.md`의 "현재 프로젝트 사실"에 있다.

## 원칙

1. 공식 fixture는 `tests/fixtures/<fixture_name>/` 아래에서만 관리한다.
2. 저장소 루트의 `sample.hwp`, `sample.hwpx`, `sample.*` 출력물은 로컬 개발/수동 확인용이며 커밋하지 않는다.
3. 가능하면 같은 의미 내용의 `input.hwp`와 `input.hwpx`를 쌍으로 둔다.
4. 기대값은 "이상적 미래 상태"가 아니라 "현재 코드가 보장해야 하는 상태"부터 고정한다.
5. 이미지 bytes 전체, HWP 내부 ID, warning 순서처럼 흔들리는 값은 전체 비교보다 부분 비교를 우선한다.
6. 기본 SVG fixture는 현재 CLI `--to svg`(semantic/plain-text exporter) 결과를 기준으로 한다.
7. RenderSnapshot 기반 visual SVG와 visual-check artifact는 기본 SVG golden과 섞지 않고 `diagnostics/` 또는 별도 smoke로 분리한다.
8. HWPX paired fixture는 매칭 HWP fixture와 같은 feature-level assertion을 통과할 때만 추가한다. 부분 통과를 위해 assertion을 약화하지 않는다. (배경: `docs/STATUS.md`의 거부된 synthetic HWPX 시도)

## 권장 구조

```text
tests/fixtures/<fixture_name>/
  input.hwp
  input.hwpx
  notes.md
  expected/
    bridge.json
    txt.txt
    markdown.md
    html.html
    svg.svg
  diagnostics/
    render-snapshot.svg
    render-snapshot-summary.json
```

`expected/svg.svg`는 CLI `--to svg` 결과(semantic SVG)를 의미한다. `diagnostics/`는 RenderSnapshot 경로를 검증하는 fixture에만 둔다.

`tests/fixture_smoke.rs`는 `input.hwp`/`input.hwpx`를 자동 발견해 기본 smoke를 실행한다. 입력 파일이 없으면 테스트는 준비 상태로 통과한다.

## Assertion 우선순위

1. `bridge smoke`: `bridge::rhwp::read_document`가 HWP/HWPX에서 성공하는지.
2. `feature assertion`: 핵심 IR field만 비교 (전체 JSON dump 비교 지양).
3. `export smoke`: `txt/json/markdown/html/svg` export가 모두 성공하는지.
4. `golden comparison`: 출력이 결정적인 fixture에 한해 제한적으로.
5. `diagnostics`: visual/render path만 별도 비교.

현재 `tests/fixture_smoke.rs`는 full golden 대신 feature-level assertion을 우선한다. "완벽한 출력 동일성"이 아니라 "중요 정보가 조용히 사라지지 않는다"는 최소 보장이다.

## Fixture별 계획

| Fixture | 우선순위 | 포함 요소 | 문서 내용 | bridge 핵심 assert | exporter 핵심 assert |
| --- | --- | --- | --- | --- | --- |
| `basic_text` | P0 | text, paragraph | 3~5문단, 빈 문단 1개, 줄바꿈, 탭, 한글/영문/숫자 혼합 | section 1개, 빈 문단 포함, `Inline::Text/LineBreak/Tab` | 모든 형식에 텍스트가 보이는지 |
| `style` | P0 | style | bold/italic/underline/strike, font family/size, 전경/배경색, 정렬, spacing, indent | `TextStyle`, `ParagraphStyle`, `style_ref`, `StyleSheet` | JSON/HTML golden, Markdown은 bold/italic/strike만 |
| `table` | P0 | table | 2x2~3x2 단순 표, 각 셀 단일 문단 | `Block::Table`, row/cell count, nested paragraph | HTML `<table>`, Markdown simple table, TXT/SVG fallback |
| `merged_table` | P0 | merged cell | row span 1개, col span 1개 | 해당 cell의 `row_span`/`col_span` | HTML `rowspan`/`colspan`, Markdown plain text fallback |
| `image` | P0 | image, resource | png/jpg 1개, alt, 가능하면 caption | `Block::Image`, `ImageResource`, id, extension/media type, bytes 비어있지 않음, dimensions | JSON resource 보존, HTML/Markdown `<stem>_assets/images/<id>.<ext>` 참조와 asset 파일 존재, TXT/SVG fallback |
| `link_list` | P1 | link, list | hyperlink field 1개 + control 1개, bullet/ordered/nested/restart | `Inline::Link`, URL/label, `ListInfo.kind/level/number`, marker 비어있지 않음 | HTML/Markdown URL 유지, TXT/SVG prefix/fallback |
| `note_header_footer` | P1 | header/footer, note | header/footer 각 1개, 본문 footnote/endnote 각 1개 | `Section.headers/footers`, `NoteStore`, kind/id, trailing note ref append warning | HTML/Markdown note section, TXT/SVG 선형화 |
| `equation_shape_chart` | P2 | equation, shape, chart | equation 1개, shape 2~3개, chart 1개 | equation `Block::Equation`+script/fallback, shape placeholder, chart smoke | `[equation: ...]`/`[shape: ...]`/`[chart: ...]` fallback 현재 상태 기록 |
| `kitchen_sink` | P2 | unknown 포함 전체 | 모든 요소 혼합 | block 종류/note/resource/warning 존재, silent drop 후보 점검 | 모든 exporter smoke + 일부 핵심 문자열 |

## Fixture별 세부 메모

- `basic_text`: HWP/HWPX를 같은 내용으로 저장. 빈 문단은 `Paragraph { inlines: [] }`로 보존한다. 정상 parse 경로만 다룸.
- `style`: 환경에서 흔들리지 않는 기본 글꼴 사용. 전경/배경색은 분명히 다른 값. HTML golden은 전체 비교보다 핵심 declaration 포함 여부.
- `table`: Markdown path 검증 위해 단순 구조 유지. 셀 안 다중 문단/이미지는 `kitchen_sink`로.
- `merged_table`: row/col span 각 최소 1회. Markdown은 fallback 문자열이 현재 기대값.
- `image`: resource bytes 전체를 golden으로 박제하지 말고 id/extension/media_type/`bytes.len() > 0`만. HTML/Markdown은 `<stem>_assets/images/`에 쓰고 `<stem>_assets/images/<resource_file_name>`로 참조.
- `link_list`: field-range 1개 + trailing control 1개 둘 다. ordered list는 restart 보이는 최소 케이스. bullet glyph는 exact char보다 non-empty/normalized marker 비교.
- `note_header_footer`: note ref는 문단 끝 append 경고가 핵심이므로 warning assert 필수. odd/even placement 있으면 둘 다.
- `equation_shape_chart`: equation은 bridge가 `PlainText`로만 넣는 현재 상태 고정. chart는 bridge 미지원이므로 처음엔 smoke + 현재 관찰 기록.
- `kitchen_sink`: 한 요소씩 엄격 비교보다 "전체 문서가 끝까지 parse/export 되는가" 통합 회귀용. unknown/ignored control 후보가 있으면 `notes.md`에 관찰된 손실 기록.

## Bridge stats expectation

fixture별로 안정적인 개수 지표를 고정하려면 `expected/` 아래에 bridge stats expectation을 둔다.

```text
tests/fixtures/<fixture_name>/expected/bridge-stats.json
tests/fixtures/<fixture_name>/expected/bridge-stats.hwp.json
tests/fixtures/<fixture_name>/expected/bridge-stats.hwpx.json
```

우선순위: 확장자별 파일이 있으면 그것을, 없고 `bridge-stats.json`이 있으면 공통 기대값으로, 둘 다 없으면 준비 상태로 넘어간다.

포함 지표 예: section/body block/header/footer/note count, paragraph/table/row/cell count, image/equation/shape/chart/unknown block count, text run/line break/tab/link/note ref count, resource/image resource/binary resource count, warning count.

원칙: 실제 문서 fixture가 추가된 뒤 작성한다. stats가 바뀌면 즉시 expected를 고치지 말고 개선인지 회귀인지 먼저 판단한다. 복잡한 fixture는 전체 JSON golden보다 stats + feature assertion을 우선한다.

갱신:

```bash
# bash
HWP_CONVERT_UPDATE_FIXTURE_STATS=1 cargo test --test fixture_smoke official_fixtures_match_expected_bridge_stats
```

```powershell
# PowerShell
$env:HWP_CONVERT_UPDATE_FIXTURE_STATS='1'; cargo test --test fixture_smoke official_fixtures_match_expected_bridge_stats
```

이 명령은 입력 확장자별 expected를 쓴다. 기본 `cargo test`는 expected 파일을 만들거나 고치지 않는다.

## 완료 기준

- 최소 P0 fixture 5개가 HWP/HWPX 쌍으로 준비된다.
- 각 fixture마다 bridge assert 1세트와 exporter smoke 1세트가 있다.
- `equation_shape_chart`와 `kitchen_sink`는 미지원 영역을 숨기지 않고 현재 동작을 기록한다.
