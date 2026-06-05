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

`tests/fixture_smoke.rs`는 `tests/fixtures/<fixture_name>/input.hwp`와
`tests/fixtures/<fixture_name>/input.hwpx`를 자동으로 발견해 기본 smoke를 실행한다.
아직 fixture 입력 파일이 없으면 테스트는 준비 상태로 통과한다.

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

## Bridge stats expectation

fixture별로 안정적인 개수 지표를 고정하고 싶으면 `expected/` 아래에 bridge stats expectation을 둘 수 있다.

```text
tests/fixtures/<fixture_name>/expected/bridge-stats.json
tests/fixtures/<fixture_name>/expected/bridge-stats.hwp.json
tests/fixtures/<fixture_name>/expected/bridge-stats.hwpx.json
```

확장자별 파일이 있으면 해당 입력에만 적용하고, 없으면 `bridge-stats.json`을 공통 기대값으로 사용한다.

이 expected는 전체 `Document IR` golden이 아니라 문단/표/이미지/링크/warning 같은 개수 지표를 고정하기 위한 장치다. stats가 바뀌면 기대값을 바로 고치지 말고 변환 정확도 개선인지 회귀인지 먼저 판단한다.

expected 파일 생성 또는 갱신:

```bash
HWP_CONVERT_UPDATE_FIXTURE_STATS=1 cargo test --test fixture_smoke official_fixtures_match_expected_bridge_stats
```

이 명령은 기본적으로 `bridge-stats.hwp.json`과 `bridge-stats.hwpx.json`처럼 입력 확장자별 expected를 쓴다. 일반 `cargo test`에서는 expected 파일을 생성하거나 수정하지 않는다.
