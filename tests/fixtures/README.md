# Bridge Coverage Fixtures

이 디렉터리는 HWP/HWPX bridge coverage를 검증하기 위한 공식 fixture 위치다.
공식 fixture는 `tests/fixtures` 아래에서만 관리한다.

## 관리 원칙

1. 공식 fixture는 `tests/fixtures/<fixture_name>/` 아래에 둔다.
2. 저장소 루트의 `sample.hwp`, `sample.hwpx`, `sample.*` 출력물은 로컬 개발과 수동 확인용이다. 공식 fixture가 아니며 커밋하지 않는다.
3. 초기 테스트는 완전한 golden snapshot 비교보다 feature-level assertion을 우선한다.
4. golden file은 출력이 충분히 안정적이고 비교 가치가 있을 때만 추가한다.
5. fixture 입력은 가능하면 같은 의미의 `input.hwp`와 `input.hwpx` 쌍으로 둔다.

## 확장 순서

fixture는 다음 순서로 확장한다.

1. `basic_text`
2. `style`
3. `table`
4. `merged_table`
5. `image`
6. `link_list`
7. `note_header_footer`
8. `equation_shape_chart`
9. `kitchen_sink`

`basic_text`가 첫 fixture다. 이 fixture에서 HWP/HWPX parse smoke, bridge feature assertion, exporter smoke의 기본 형태를 먼저 고정한다.

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

`expected/svg.svg`는 현재 CLI의 `--to svg` 결과를 의미한다. 이 SVG는 semantic/plain-text exporter 기준이며 RenderSnapshot visual SVG가 아니다.

RenderSnapshot 기반 visual SVG와 `write_render_snapshot_visual_check` artifact는 일반 exporter golden과 섞지 않는다. 필요한 경우 `diagnostics/` 아래 별도 fixture 또는 diagnostics smoke로 분리한다.

## Assertion 우선순위

초기 bridge coverage 테스트는 다음 순서를 따른다.

1. `bridge smoke`: `bridge::rhwp::read_document`가 HWP/HWPX에서 성공하는지 확인한다.
2. `feature assertion`: fixture의 핵심 기능이 IR에 들어왔는지 필요한 필드만 확인한다.
3. `export smoke`: `txt`, `json`, `markdown`, `html`, `svg` exporter가 끝까지 성공하는지 확인한다.
4. `golden comparison`: 출력이 안정적인 fixture에 한해 제한적으로 추가한다.

예를 들어 `image` fixture는 resource bytes 전체 비교보다 `Block::Image`, `ImageResource`, extension/media type, `bytes.len() > 0` 같은 feature-level assertion을 우선한다.
