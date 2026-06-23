# basic_text fixture

## 목적

가장 기본적인 텍스트 변환 기준점이다. HWP/HWPX 입력에서 일반 문단, 한국어 텍스트, 영문/숫자 혼합 텍스트, 문단 내부 줄바꿈, 문단 내부 탭이 Document IR로 보존되는지 확인한다.

## 현재 입력

- `input.hwp`: 준비됨
- `input.hwpx`: 준비됨

`input.hwpx`는 같은 fixture의 `input.hwp`를 rHWP로 파싱한 뒤 rHWP HWPX serializer로 직렬화해 만든 동등 의미 문서다. 두 입력 모두 `bridge::rhwp::read_document` 경로를 통과하며, feature assertion과 exporter smoke를 함께 실행한다.

## 포함된 기능

- 한국어 문단 1개
- 영문/숫자 혼합 문단 1개
- 문단 내부 line break 1개
- 문단 내부 tab 1개
- 빈 문단 1개

## 검증 기준

`tests/fixture_smoke.rs`의 `assert_basic_text_fixture`가 다음을 확인한다.

- 비어 있지 않은 문단 4개와 빈 문단 1개가 `Block::Paragraph`로 생성된다.
- 한국어 문단 텍스트가 보존된다.
- `English 123 mixed text`가 보존된다.
- 문단 내부 줄바꿈이 `Inline::LineBreak`로 보존된다.
- 문단 내부 탭이 `Inline::Tab`으로 보존된다.
- 빈 문단은 `Paragraph { inlines: [] }`로 보존된다.

## 현재 관찰값

HWP와 HWPX 모두 현재 bridge stats가 같은 구조를 가진다.

- `sections`: 1
- `body_blocks`: 5
- `paragraphs`: 5
- `text_runs`: 6
- `line_breaks`: 1
- `tabs`: 1
- `warnings`: 1

기대값은 다음 파일로 고정한다.

- `expected/bridge-stats.hwp.json`
- `expected/bridge-stats.hwpx.json`
